use crate::ecs::World;
use rayon::prelude::*;

pub trait System: Send {
    fn run(&mut self, world: &mut World, delta_seconds: f32);
}

pub trait ParallelSystem: Send {
    fn run(&mut self, world: &World, delta_seconds: f32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    Startup,
    Simulation,
    Render,
    Editor,
}

impl Stage {
    pub const fn ordered() -> [Stage; 4] {
        [
            Stage::Startup,
            Stage::Simulation,
            Stage::Render,
            Stage::Editor,
        ]
    }
}

struct SystemEntry {
    name: &'static str,
    system: Box<dyn System>,
}

struct ParallelEntry {
    name: &'static str,
    system: Box<dyn ParallelSystem>,
}

struct StageBucket {
    stage: Stage,
    sequential: Vec<SystemEntry>,
    parallel: Vec<ParallelEntry>,
}

impl StageBucket {
    fn new(stage: Stage) -> Self {
        Self {
            stage,
            sequential: Vec::new(),
            parallel: Vec::new(),
        }
    }
}

pub struct Scheduler {
    world: World,
    buckets: Vec<StageBucket>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(World::default())
    }
}

impl Scheduler {
    pub fn new(world: World) -> Self {
        let buckets = Stage::ordered().into_iter().map(StageBucket::new).collect();
        Self { world, buckets }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn add_system<S>(&mut self, stage: Stage, name: &'static str, system: S)
    where
        S: System + 'static,
    {
        let bucket = self.bucket_mut(stage);
        bucket.sequential.push(SystemEntry {
            name,
            system: Box::new(system),
        });
    }

    pub fn add_system_fn<F>(&mut self, stage: Stage, name: &'static str, func: F)
    where
        F: FnMut(&mut World, f32) + Send + 'static,
    {
        self.add_system(stage, name, FnSystem { func });
    }

    pub fn add_parallel_system<P>(&mut self, stage: Stage, name: &'static str, system: P)
    where
        P: ParallelSystem + 'static,
    {
        let bucket = self.bucket_mut(stage);
        bucket.parallel.push(ParallelEntry {
            name,
            system: Box::new(system),
        });
    }

    pub fn add_parallel_system_fn<F>(&mut self, stage: Stage, name: &'static str, func: F)
    where
        F: Fn(&World, f32) + Send + Sync + 'static,
    {
        self.add_parallel_system(stage, name, FnParallelSystem { func });
    }

    pub fn tick(&mut self, delta_seconds: f32) {
        for bucket in &mut self.buckets {
            for entry in &mut bucket.sequential {
                println!(
                    "[scheduler::{:?}] running system {}",
                    bucket.stage, entry.name
                );
                entry.system.run(&mut self.world, delta_seconds);
            }

            if !bucket.parallel.is_empty() {
                let world_ref = &self.world;
                let stage = bucket.stage;
                bucket.parallel.par_iter_mut().for_each(|entry| {
                    println!(
                        "[scheduler::{:?}] running parallel system {}",
                        stage, entry.name
                    );
                    entry.system.run(world_ref, delta_seconds);
                });
            }
        }
    }

    fn bucket_mut(&mut self, stage: Stage) -> &mut StageBucket {
        self.buckets
            .iter_mut()
            .find(|bucket| bucket.stage == stage)
            .expect("stage bucket must exist")
    }
}

struct FnSystem<F: FnMut(&mut World, f32) + Send + 'static> {
    func: F,
}

impl<F> System for FnSystem<F>
where
    F: FnMut(&mut World, f32) + Send + 'static,
{
    fn run(&mut self, world: &mut World, delta_seconds: f32) {
        (self.func)(world, delta_seconds);
    }
}

struct FnParallelSystem<F: Fn(&World, f32) + Send + Sync + 'static> {
    func: F,
}

impl<F> ParallelSystem for FnParallelSystem<F>
where
    F: Fn(&World, f32) + Send + Sync + 'static,
{
    fn run(&mut self, world: &World, delta_seconds: f32) {
        (self.func)(world, delta_seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    #[test]
    fn scheduler_invokes_registered_systems() {
        let mut scheduler = Scheduler::default();
        scheduler.world_mut().register_component::<u32>();
        let entity = scheduler.world_mut().spawn();
        scheduler
            .world_mut()
            .insert(entity, 0u32)
            .expect("component storage");

        scheduler.add_system_fn(Stage::Simulation, "increment", move |world, _delta| {
            if let Some(value) = world.get_mut::<u32>(entity) {
                *value += 1;
            }
        });

        scheduler.tick(0.016);

        assert_eq!(scheduler.world().get::<u32>(entity).copied(), Some(1));
    }

    #[test]
    fn stages_execute_in_order() {
        let mut scheduler = Scheduler::default();
        let order = Arc::new(Mutex::new(Vec::new()));

        {
            let order = order.clone();
            scheduler.add_system_fn(Stage::Startup, "startup", move |_, _| {
                order.lock().unwrap().push("startup");
            });
        }

        {
            let order = order.clone();
            scheduler.add_system_fn(Stage::Simulation, "simulation", move |_, _| {
                order.lock().unwrap().push("simulation");
            });
        }

        {
            let order = order.clone();
            scheduler.add_system_fn(Stage::Render, "render", move |_, _| {
                order.lock().unwrap().push("render");
            });
        }

        scheduler.tick(0.016);

        let order = order.lock().unwrap().clone();
        assert_eq!(order, vec!["startup", "simulation", "render"]);
    }

    #[test]
    fn parallel_systems_run_without_blocking() {
        let mut scheduler = Scheduler::default();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_handle = counter.clone();

        scheduler.add_parallel_system_fn(Stage::Editor, "parallel", move |_, _| {
            counter_handle.fetch_add(1, Ordering::SeqCst);
        });

        scheduler.tick(0.016);

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
