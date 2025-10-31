mod commands;
pub mod schedule;

use self::commands::CommandPipeline;
use crate::ecs::World;
use crate::editor::telemetry::{FrameTelemetry, TelemetryReplicator, TelemetrySurface};
use crate::network::EntityHandle;
use crate::render::{BackendKind, GpuBackend, NullGpuBackend, Renderer, RendererConfig};
#[cfg(feature = "vr-openxr")]
use crate::vr::openxr::OpenXrInputProvider;
use crate::vr::{
    ControllerState, NullVrBridge, SimulatedInputProvider, TrackedPose, VrBridge, VrInputProvider,
};
use schedule::{Scheduler, Stage, System};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const DEFAULT_MAX_FRAMES: u32 = 3;

pub struct Engine {
    scheduler: Scheduler,
    renderer: Renderer,
    target_frame_time: f32,
    max_frames: u32,
    frame_stats_entity: Option<crate::ecs::Entity>,
    telemetry_entity: Option<crate::ecs::Entity>,
    input_provider: Arc<Mutex<Box<dyn VrInputProvider>>>,
    command_pipeline: Arc<Mutex<CommandPipeline>>,
}

impl Engine {
    pub fn new() -> Self {
        let mut config = RendererConfig::default();
        if cfg!(feature = "render-wgpu") {
            config.backend = BackendKind::Wgpu;
        }
        Self::with_renderer_config(config)
    }

    pub fn with_renderer_config(config: RendererConfig) -> Self {
        let scheduler = Scheduler::default();
        let renderer = Self::build_renderer(config);
        let input_provider = build_input_provider();
        let command_pipeline = Arc::new(Mutex::new(CommandPipeline::new()));

        let mut engine = Self {
            scheduler,
            renderer,
            target_frame_time: 1.0 / 60.0,
            max_frames: DEFAULT_MAX_FRAMES,
            frame_stats_entity: None,
            telemetry_entity: None,
            input_provider,
            command_pipeline,
        };

        engine.register_core_systems();
        engine
    }

    pub fn with_backend(backend: BackendKind) -> Self {
        let mut config = RendererConfig::default();
        config.backend = backend;
        Self::with_renderer_config(config)
    }

    pub fn add_system<S>(&mut self, stage: Stage, name: &'static str, system: S)
    where
        S: System + 'static,
    {
        self.scheduler.add_system(stage, name, system);
    }

    pub fn add_system_fn<F>(&mut self, stage: Stage, name: &'static str, func: F)
    where
        F: FnMut(&mut World, f32) + Send + 'static,
    {
        self.scheduler.add_system_fn(stage, name, func);
    }

    pub fn add_parallel_system_fn<F>(&mut self, stage: Stage, name: &'static str, func: F)
    where
        F: Fn(&World, f32) + Send + Sync + 'static,
    {
        self.scheduler.add_parallel_system_fn(stage, name, func);
    }

    pub fn configure_max_frames(&mut self, frames: u32) {
        self.max_frames = frames.max(1);
    }

    pub fn run(&mut self) {
        let mut last_frame = Instant::now();
        for _ in 0..self.max_frames {
            let now = Instant::now();
            let raw_delta = now.duration_since(last_frame).as_secs_f32();
            let delta_seconds = if raw_delta == 0.0 {
                self.target_frame_time
            } else {
                raw_delta
            };
            last_frame = now;

            self.scheduler.tick(delta_seconds);
            self.update_frame_diagnostics();

            if let Err(err) = self.renderer.render(delta_seconds) {
                eprintln!("[engine] render error: {err}");
            }
        }
    }

    pub fn world(&self) -> &crate::ecs::World {
        self.scheduler.world()
    }

    pub fn telemetry_entity(&self) -> Option<crate::ecs::Entity> {
        self.telemetry_entity
    }

    pub fn world_mut(&mut self) -> &mut crate::ecs::World {
        self.scheduler.world_mut()
    }

    fn register_core_systems(&mut self) {
        {
            let world = self.scheduler.world_mut();
            world.register_component::<FrameStats>();
            world.register_component::<Transform>();
            world.register_component::<Velocity>();
            world.register_component::<EditorSelection>();
            world.register_component::<TrackedPose>();
            world.register_component::<ControllerState>();
            world.register_component::<TelemetrySurface>();
            world.register_component::<TelemetryReplicator>();
        }

        let stats_entity = {
            let world = self.scheduler.world_mut();
            initialize_frame_stats(world)
        };
        self.frame_stats_entity = Some(stats_entity);

        let telemetry_entity = {
            let world = self.scheduler.world_mut();
            initialize_telemetry(world)
        };
        self.telemetry_entity = Some(telemetry_entity);

        let head_entity = {
            let world = self.scheduler.world_mut();
            initialize_head_pose(world)
        };

        let left_controller_entity = {
            let world = self.scheduler.world_mut();
            initialize_controller(world, true)
        };

        let right_controller_entity = {
            let world = self.scheduler.world_mut();
            initialize_controller(world, false)
        };

        let actor_entity = {
            let world = self.scheduler.world_mut();
            initialize_actor(world)
        };

        let editor_entity = {
            let world = self.scheduler.world_mut();
            initialize_editor_state(world, actor_entity)
        };

        let input_source = Arc::clone(&self.input_provider);
        self.add_system_fn(Stage::Simulation, "update_vr_input", move |world, delta| {
            let sample = {
                let mut provider = input_source
                    .lock()
                    .expect("vr input provider mutex should not poison");
                provider.sample(delta)
            };

            if let Some(pose) = world.get_mut::<TrackedPose>(head_entity) {
                *pose = sample.head;
            }

            if let Some(state) = world.get_mut::<ControllerState>(left_controller_entity) {
                *state = sample.left;
            }

            if let Some(state) = world.get_mut::<ControllerState>(right_controller_entity) {
                *state = sample.right;
            }
        });

        self.add_system_fn(
            Stage::Simulation,
            "integrate_velocity",
            move |world, delta| {
                let velocity = world.get::<Velocity>(actor_entity).copied();
                if let (Some(transform), Some(velocity)) =
                    (world.get_mut::<Transform>(actor_entity), velocity)
                {
                    transform.integrate(&velocity, delta);
                }
            },
        );

        self.add_system_fn(Stage::Editor, "frame_stats", move |world, delta| {
            let actor_position = world
                .get::<Transform>(actor_entity)
                .map(|transform| transform.position);
            let left_trigger = world
                .get::<ControllerState>(left_controller_entity)
                .map(|state| state.trigger)
                .unwrap_or_default();
            let right_trigger = world
                .get::<ControllerState>(right_controller_entity)
                .map(|state| state.trigger)
                .unwrap_or_default();
            if let Some(stats) = world.get_mut::<FrameStats>(stats_entity) {
                stats.frames += 1;
                stats.total_time += delta;
                stats.average_frame_time = stats.total_time / stats.frames as f32;

                if let Some(position) = actor_position {
                    stats.last_actor_position = position;
                }

                stats.controller_trigger[0] = left_trigger;
                stats.controller_trigger[1] = right_trigger;

                println!(
                    "[engine] frame {} avg {:.4}s pos {:?} L:{:.2} R:{:.2}",
                    stats.frames,
                    stats.average_frame_time,
                    stats.last_actor_position,
                    stats.controller_trigger[0],
                    stats.controller_trigger[1]
                );
                println!(
                    "           stage timings ms {:?} (seq {:?}, par {:?}, avg {:?})",
                    stats.stage_durations_ms,
                    stats.stage_sequential_ms,
                    stats.stage_parallel_ms,
                    stats.stage_rolling_ms
                );
                for (stage, &violation) in Stage::ordered()
                    .iter()
                    .zip(stats.stage_read_only_violation.iter())
                {
                    if violation {
                        println!(
                            "           warning: {:?} stage executed exclusive systems (total {} violations)",
                            stage,
                            stats.stage_violation_count[stage.index()]
                        );
                    }
                }
            }
        });

        let pipeline_handle = Arc::clone(&self.command_pipeline);
        self.add_system_fn(Stage::Editor, "cycle_selection", move |world, _delta| {
            if let Some(selection) = world.get_mut::<EditorSelection>(editor_entity) {
                selection.frames_since_change += 1;
                if selection.frames_since_change >= selection.highlight_interval {
                    selection.frames_since_change = 0;
                    selection.highlight_active = !selection.highlight_active;

                    if let Some(primary) = selection.primary {
                        let handle = EntityHandle::from(primary);
                        if let Ok(mut pipeline) = pipeline_handle.lock() {
                            if let Err(err) = pipeline
                                .record_selection_highlight(handle, selection.highlight_active)
                            {
                                eprintln!("[commands] failed to record highlight command: {err}");
                            }
                        }
                    }
                }
            }
        });

        self.add_parallel_system_fn(Stage::Editor, "editor_debug_view", move |world, _| {
            if let Some(selection) = world.get::<EditorSelection>(editor_entity) {
                if let Some(entity) = selection.primary {
                    if let Some(transform) = world.get::<Transform>(entity) {
                        println!(
                            "[editor] selection {:?} transform {:?} highlight {}",
                            entity, transform.position, selection.highlight_active
                        );
                    }
                }
            }
        });
    }

    fn build_renderer(config: RendererConfig) -> Renderer {
        let backend = Self::create_backend(config.backend);
        let vr: Box<dyn VrBridge> = Box::new(NullVrBridge::default());
        Renderer::new(config, backend, vr)
    }

    fn create_backend(kind: BackendKind) -> Box<dyn GpuBackend> {
        match kind {
            BackendKind::Null => Box::new(NullGpuBackend::default()),
            BackendKind::Wgpu => {
                #[cfg(feature = "render-wgpu")]
                {
                    match crate::render::wgpu_backend::WgpuBackend::initialize() {
                        Ok(backend) => Box::new(backend) as Box<dyn GpuBackend>,
                        Err(err) => {
                            eprintln!(
                                "[engine] failed to initialize wgpu backend ({err}); falling back to Null"
                            );
                            Box::new(NullGpuBackend::default())
                        }
                    }
                }

                #[cfg(not(feature = "render-wgpu"))]
                {
                    eprintln!(
                        "[engine] wgpu backend requested but 'render-wgpu' feature is disabled; falling back to Null"
                    );
                    Box::new(NullGpuBackend::default())
                }
            }
        }
    }
}

#[derive(Default)]
struct FrameStats {
    frames: u64,
    total_time: f32,
    average_frame_time: f32,
    last_actor_position: [f32; 3],
    stage_durations_ms: [f32; Stage::count()],
    stage_sequential_ms: [f32; Stage::count()],
    stage_parallel_ms: [f32; Stage::count()],
    stage_rolling_ms: [f32; Stage::count()],
    stage_read_only_violation: [bool; Stage::count()],
    stage_violation_count: [u32; Stage::count()],
    profiling_samples: u64,
    controller_trigger: [f32; 2],
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Transform {
    position: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 1.6, 0.0],
        }
    }
}

impl Transform {
    fn integrate(&mut self, velocity: &Velocity, delta: f32) {
        for (value, vel) in self.position.iter_mut().zip(velocity.linear.iter()) {
            *value += *vel * delta;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Velocity {
    linear: [f32; 3],
}

impl Default for Velocity {
    fn default() -> Self {
        Self {
            linear: [0.2, 0.0, 0.1],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EditorSelection {
    primary: Option<crate::ecs::Entity>,
    frames_since_change: u32,
    highlight_interval: u32,
    highlight_active: bool,
}

impl Default for EditorSelection {
    fn default() -> Self {
        Self {
            primary: None,
            frames_since_change: 0,
            highlight_interval: 120,
            highlight_active: true,
        }
    }
}

fn initialize_frame_stats(world: &mut World) -> crate::ecs::Entity {
    let entity = world.spawn();
    world
        .insert(entity, FrameStats::default())
        .expect("frame stats component should insert");
    entity
}

fn initialize_telemetry(world: &mut World) -> crate::ecs::Entity {
    let entity = world.spawn();
    world
        .insert(entity, TelemetrySurface::default())
        .expect("telemetry surface component should insert");
    world
        .insert(entity, TelemetryReplicator::default())
        .expect("telemetry replicator component should insert");
    entity
}

fn initialize_head_pose(world: &mut World) -> crate::ecs::Entity {
    let entity = world.spawn();
    world
        .insert(entity, TrackedPose::default())
        .expect("head pose component");
    entity
}

fn initialize_controller(world: &mut World, left: bool) -> crate::ecs::Entity {
    let entity = world.spawn();
    let mut state = ControllerState::default();
    if left {
        state.pose.position = [-0.25, 1.4, 0.3];
    } else {
        state.pose.position = [0.25, 1.4, 0.3];
    }
    world
        .insert(entity, state)
        .expect("controller state component");
    entity
}

fn initialize_actor(world: &mut World) -> crate::ecs::Entity {
    let entity = world.spawn();
    world
        .insert(entity, Transform::default())
        .expect("transform component");
    world
        .insert(entity, Velocity::default())
        .expect("velocity component");
    entity
}

fn initialize_editor_state(world: &mut World, primary: crate::ecs::Entity) -> crate::ecs::Entity {
    let mut selection = EditorSelection::default();
    selection.primary = Some(primary);
    let entity = world.spawn();
    world
        .insert(entity, selection)
        .expect("editor selection component");
    entity
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn build_input_provider() -> Arc<Mutex<Box<dyn VrInputProvider>>> {
    #[cfg(feature = "vr-openxr")]
    {
        match OpenXrInputProvider::initialize() {
            Ok(provider) => {
                println!("[engine] OpenXR input provider initialized");
                return Arc::new(Mutex::new(Box::new(provider) as Box<dyn VrInputProvider>));
            }
            Err(err) => {
                eprintln!("[engine] OpenXR input unavailable; falling back to simulation: {err}");
            }
        }
    }

    Arc::new(Mutex::new(
        Box::new(SimulatedInputProvider::default()) as Box<dyn VrInputProvider>
    ))
}

impl Engine {
    fn update_frame_diagnostics(&mut self) {
        let profile = self.scheduler.last_profile().clone();
        let mut telemetry_sample = None;

        if let Some(stats_entity) = self.frame_stats_entity {
            if let Some(stats) = self
                .scheduler
                .world_mut()
                .get_mut::<FrameStats>(stats_entity)
            {
                let prev_samples = stats.profiling_samples;
                let new_samples = prev_samples.saturating_add(1);
                for stage in Stage::ordered() {
                    let index = stage.index();
                    if let Some(stage_profile) = profile.stage(stage) {
                        stats.stage_durations_ms[index] = stage_profile.total_ms();
                        stats.stage_sequential_ms[index] = stage_profile.sequential_ms();
                        stats.stage_parallel_ms[index] = stage_profile.parallel_ms();
                        stats.stage_read_only_violation[index] = stage_profile.read_only_violation;

                        let prev_average = stats.stage_rolling_ms[index];
                        stats.stage_rolling_ms[index] = if prev_samples == 0 {
                            stage_profile.total_ms()
                        } else {
                            let delta = stage_profile.total_ms() - prev_average;
                            prev_average + delta / (new_samples as f32)
                        };

                        if stage_profile.read_only_violation {
                            stats.stage_violation_count[index] =
                                stats.stage_violation_count[index].saturating_add(1);
                        }
                    }
                }

                stats.profiling_samples = new_samples;

                telemetry_sample = Some(FrameTelemetry::from_stage_arrays(
                    stats.frames,
                    stats.average_frame_time,
                    &stats.stage_durations_ms,
                    &stats.stage_sequential_ms,
                    &stats.stage_parallel_ms,
                    &stats.stage_rolling_ms,
                    &stats.stage_read_only_violation,
                    &stats.stage_violation_count,
                    stats.controller_trigger,
                ));
            }
        }

        if let (Some(entity), Some(sample)) = (self.telemetry_entity, telemetry_sample) {
            let world = self.scheduler.world_mut();
            let mut latest_to_publish = None;

            if let Some(surface) = world.get_mut::<TelemetrySurface>(entity) {
                if surface.record(sample) {
                    latest_to_publish = surface.latest().cloned();
                }
            }

            if let Some(latest) = latest_to_publish {
                if let Some(replicator) = world.get_mut::<TelemetryReplicator>(entity) {
                    replicator.publish(entity, &latest);
                }
            }
        }

        if let Ok(mut pipeline) = self.command_pipeline.lock() {
            for batch in pipeline.drain_batches() {
                println!(
                    "[commands] batch {} entries {}",
                    batch.sequence,
                    batch.entries.len()
                );
            }
        }
    }
}

crate::register_component_types!(FrameStats, Transform, Velocity, EditorSelection);
