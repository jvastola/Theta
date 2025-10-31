use crate::ecs::World;
use rayon::prelude::*;
use std::time::{Duration, Instant};

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

    pub const fn count() -> usize {
        4
    }

    pub fn index(self) -> usize {
        match self {
            Stage::Startup => 0,
            Stage::Simulation => 1,
            Stage::Render => 2,
            Stage::Editor => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Stage::Startup => "Startup",
            Stage::Simulation => "Simulation",
            Stage::Render => "Render",
            Stage::Editor => "Editor",
        }
    }

    fn policy(self) -> StagePolicy {
        match self {
            Stage::Startup => StagePolicy::Initialization,
            Stage::Simulation => StagePolicy::Mutation,
            Stage::Render => StagePolicy::ReadMostly,
            Stage::Editor => StagePolicy::Tooling,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StagePolicy {
    Initialization,
    Mutation,
    ReadMostly,
    Tooling,
}

#[derive(Debug, Clone, Default)]
pub struct FrameProfile {
    stages: Vec<StageProfile>,
}

impl FrameProfile {
    pub fn stages(&self) -> &[StageProfile] {
        &self.stages
    }

    pub fn stage(&self, stage: Stage) -> Option<&StageProfile> {
        self.stages.iter().find(|profile| profile.stage == stage)
    }
}

#[derive(Debug, Clone)]
pub struct StageProfile {
    pub stage: Stage,
    pub total: Duration,
    pub sequential_total: Duration,
    pub parallel_total: Duration,
    pub sequential_systems: Vec<SystemProfile>,
    pub parallel_count: usize,
    pub read_only_violation: bool,
}

impl StageProfile {
    pub fn total_ms(&self) -> f32 {
        self.total.as_secs_f64() as f32 * 1000.0
    }

    pub fn sequential_ms(&self) -> f32 {
        self.sequential_total.as_secs_f64() as f32 * 1000.0
    }

    pub fn parallel_ms(&self) -> f32 {
        self.parallel_total.as_secs_f64() as f32 * 1000.0
    }
}

#[derive(Debug, Clone)]
pub struct SystemProfile {
    pub name: &'static str,
    pub duration: Duration,
}

impl SystemProfile {
    pub fn duration_ms(&self) -> f32 {
        self.duration.as_secs_f64() as f32 * 1000.0
    }
}

const SLOW_SYSTEM_THRESHOLD_MS: f32 = 4.0;
const SLOW_STAGE_THRESHOLD_MS: f32 = 12.0;

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
    last_profile: FrameProfile,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(World::default())
    }
}

impl Scheduler {
    pub fn new(world: World) -> Self {
        let buckets = Stage::ordered().into_iter().map(StageBucket::new).collect();
        Self {
            world,
            buckets,
            last_profile: FrameProfile::default(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn last_profile(&self) -> &FrameProfile {
        &self.last_profile
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
        let mut frame_profile = FrameProfile::default();

        for bucket in &mut self.buckets {
            let stage_start = Instant::now();
            let mut sequential_profiles = Vec::with_capacity(bucket.sequential.len());
            let mut sequential_total = Duration::ZERO;

            for entry in &mut bucket.sequential {
                println!(
                    "[scheduler::{:?}] running system {}",
                    bucket.stage, entry.name
                );
                let system_start = Instant::now();
                entry.system.run(&mut self.world, delta_seconds);
                let duration = system_start.elapsed();
                sequential_total += duration;
                sequential_profiles.push(SystemProfile {
                    name: entry.name,
                    duration,
                });

                if duration.as_secs_f32() * 1000.0 > SLOW_SYSTEM_THRESHOLD_MS {
                    eprintln!(
                        "[scheduler::{:?}] warning: system {} took {:.3} ms",
                        bucket.stage,
                        entry.name,
                        duration.as_secs_f64() * 1000.0,
                    );
                }
            }

            let mut parallel_duration = Duration::ZERO;

            if !bucket.parallel.is_empty() {
                let world_ref = &self.world;
                let stage = bucket.stage;
                let parallel_start = Instant::now();
                bucket.parallel.par_iter_mut().for_each(|entry| {
                    println!(
                        "[scheduler::{:?}] running parallel system {}",
                        stage, entry.name
                    );
                    entry.system.run(world_ref, delta_seconds);
                });
                parallel_duration = parallel_start.elapsed();
            }

            let total_duration = stage_start.elapsed();
            let read_only_violation = matches!(bucket.stage.policy(), StagePolicy::ReadMostly)
                && !bucket.sequential.is_empty();

            if read_only_violation {
                eprintln!(
                    "[scheduler::{:?}] warning: stage prefers read-only systems but {} exclusive system(s) executed",
                    bucket.stage,
                    bucket.sequential.len()
                );
            }

            if total_duration.as_secs_f32() * 1000.0 > SLOW_STAGE_THRESHOLD_MS {
                eprintln!(
                    "[scheduler::{:?}] warning: stage took {:.3} ms",
                    bucket.stage,
                    total_duration.as_secs_f64() * 1000.0
                );
            }

            println!(
                "[scheduler::{:?}] stage {:.3} ms (seq {:.3} ms, par {:.3} ms, {} parallel systems)",
                bucket.stage,
                total_duration.as_secs_f64() * 1000.0,
                sequential_total.as_secs_f64() * 1000.0,
                parallel_duration.as_secs_f64() * 1000.0,
                bucket.parallel.len()
            );

            for profile in &sequential_profiles {
                println!(
                    "    [system] {} {:.3} ms",
                    profile.name,
                    profile.duration.as_secs_f64() * 1000.0
                );
            }

            frame_profile.stages.push(StageProfile {
                stage: bucket.stage,
                total: total_duration,
                sequential_total,
                parallel_total: parallel_duration,
                sequential_systems: sequential_profiles,
                parallel_count: bucket.parallel.len(),
                read_only_violation,
            });
        }

        self.last_profile = frame_profile;
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
