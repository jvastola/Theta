mod commands;
pub use self::commands::CommandMetricsSnapshot;
pub use self::commands::CommandPipeline;
pub mod schedule;
use crate::ecs::World;
use crate::editor::commands::{
    CMD_ENTITY_ROTATE, CMD_ENTITY_SCALE, CMD_ENTITY_TRANSLATE, CMD_MESH_EDGE_EXTRUDE,
    CMD_MESH_FACE_SUBDIVIDE, CMD_MESH_VERTEX_CREATE, CMD_SELECTION_HIGHLIGHT, CMD_TOOL_ACTIVATE,
    CMD_TOOL_DEACTIVATE, EdgeExtrudeCommand, EntityRotateCommand, EntityScaleCommand,
    EntityTranslateCommand, FaceSubdivideCommand, SelectionHighlightCommand, ToolActivateCommand,
    ToolDeactivateCommand, VertexCreateCommand,
};
use crate::editor::telemetry::{FrameTelemetry, TelemetryReplicator, TelemetrySurface};
use crate::editor::{CommandOutbox, CommandTransportQueue};
use crate::network::EntityHandle;
use crate::network::command_log::{CommandBatch, CommandEntry, CommandPacket, CommandScope};
#[cfg(feature = "network-quic")]
use crate::network::transport::{CommandTransport, TransportError, TransportSession};
use crate::render::{BackendKind, GpuBackend, NullGpuBackend, Renderer, RendererConfig};
#[cfg(feature = "vr-openxr")]
use crate::vr::openxr::OpenXrInputProvider;
use crate::vr::{
    ControllerState, NullVrBridge, SimulatedInputProvider, TrackedPose, VrBridge, VrInputProvider,
};
use schedule::{Scheduler, Stage, System};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
#[cfg(feature = "network-quic")]
use std::time::Duration;
use std::time::Instant;
#[cfg(feature = "network-quic")]
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime as TokioRuntime};

const DEFAULT_MAX_FRAMES: u32 = 3;

pub struct Engine {
    scheduler: Scheduler,
    renderer: Renderer,
    target_frame_time: f32,
    max_frames: u32,
    frame_stats_entity: Option<crate::ecs::Entity>,
    telemetry_entity: Option<crate::ecs::Entity>,
    command_entity: Option<crate::ecs::Entity>,
    input_provider: Arc<Mutex<Box<dyn VrInputProvider>>>,
    command_pipeline: Arc<Mutex<CommandPipeline>>,
    #[cfg(feature = "network-quic")]
    command_transport: Option<CommandTransport>,
    #[cfg(feature = "network-quic")]
    network_runtime: Option<TokioRuntime>,
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
            command_entity: None,
            input_provider,
            command_pipeline,
            #[cfg(feature = "network-quic")]
            command_transport: None,
            #[cfg(feature = "network-quic")]
            network_runtime: None,
        };

        engine.register_core_systems();
        engine
    }

    pub fn with_backend(backend: BackendKind) -> Self {
        let config = RendererConfig {
            backend,
            ..RendererConfig::default()
        };
        Self::with_renderer_config(config)
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_command_transport(&mut self, transport: CommandTransport) {
        if let Ok(mut pipeline) = self.command_pipeline.lock() {
            pipeline.attach_transport_metrics(transport.metrics_handle());
        }

        if self.network_runtime.is_none() {
            self.network_runtime = Some(
                TokioRuntimeBuilder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("create network runtime"),
            );
        }

        self.command_transport = Some(transport);
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_transport_session(&mut self, session: TransportSession) {
        self.attach_command_transport(CommandTransport::from(session));
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
            world.register_component::<CommandOutbox>();
            world.register_component::<CommandTransportQueue>();
            world.register_component::<EditorToolState>();
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
        self.command_entity = Some(editor_entity);
        {
            let world = self.scheduler.world_mut();
            world
                .insert(editor_entity, CommandOutbox::default())
                .expect("command outbox component should insert");
            world
                .insert(editor_entity, CommandTransportQueue::default())
                .expect("command transport queue should insert");
            world
                .insert(editor_entity, EditorToolState::default())
                .expect("editor tool state component should insert");
        }

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
                        if let Ok(mut pipeline) = pipeline_handle.lock()
                            && let Err(err) = pipeline
                                .record_selection_highlight(handle, selection.highlight_active)
                        {
                            eprintln!("[commands] failed to record highlight command: {err}");
                        }
                    }
                }
            }
        });

        self.add_parallel_system_fn(Stage::Editor, "editor_debug_view", move |world, _| {
            if let Some(selection) = world.get::<EditorSelection>(editor_entity)
                && let Some(entity) = selection.primary
                && let Some(transform) = world.get::<Transform>(entity)
            {
                println!(
                    "[editor] selection {:?} transform {:?} highlight {}",
                    entity, transform.position, selection.highlight_active
                );
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
            BackendKind::Null => Box::new(NullGpuBackend),
            BackendKind::Wgpu => {
                #[cfg(feature = "render-wgpu")]
                {
                    match crate::render::wgpu_backend::WgpuBackend::initialize() {
                        Ok(backend) => Box::new(backend) as Box<dyn GpuBackend>,
                        Err(err) => {
                            eprintln!(
                                "[engine] failed to initialize wgpu backend ({err}); falling back to Null"
                            );
                            Box::new(NullGpuBackend)
                        }
                    }
                }

                #[cfg(not(feature = "render-wgpu"))]
                {
                    eprintln!(
                        "[engine] wgpu backend requested but 'render-wgpu' feature is disabled; falling back to Null"
                    );
                    Box::new(NullGpuBackend)
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
    rotation: [f32; 4],
    scale: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 1.6, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EditorToolState {
    active_tool: Option<String>,
    last_lamport: Option<u64>,
}

impl EditorToolState {
    fn activate(&mut self, tool_id: String, lamport: u64) {
        self.active_tool = Some(tool_id);
        self.last_lamport = Some(lamport);
    }

    fn deactivate(&mut self, lamport: u64) {
        self.active_tool = None;
        self.last_lamport = Some(lamport);
    }

    fn matches_active(&self, tool_id: &str) -> bool {
        self.active_tool.as_deref() == Some(tool_id)
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
    let selection = EditorSelection {
        primary: Some(primary),
        ..EditorSelection::default()
    };
    let entity = world.spawn();
    world
        .insert(entity, selection)
        .expect("editor selection component");
    entity
}

fn sanitize_scale(mut scale: [f32; 3]) -> [f32; 3] {
    for axis in scale.iter_mut() {
        if !axis.is_finite() {
            *axis = 1.0;
            continue;
        }

        if axis.abs() < 0.000_1 {
            *axis = if *axis >= 0.0 { 0.000_1 } else { -0.000_1 };
        }
    }
    scale
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
        let mut command_metrics_snapshot: Option<CommandMetricsSnapshot> = None;

        #[cfg(feature = "network-quic")]
        self.poll_remote_commands();

        if let Some(stats_entity) = self.frame_stats_entity
            && let Some(stats) = self
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

        if let Ok(mut pipeline) = self.command_pipeline.lock() {
            let packets = pipeline.drain_packets();
            if !packets.is_empty() {
                let mut decoded_batches: Vec<CommandBatch> = Vec::with_capacity(packets.len());
                for packet in &packets {
                    match packet.decode() {
                        Ok(batch) => decoded_batches.push(batch),
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode command packet seq {}: {err}",
                                packet.sequence
                            );
                        }
                    }
                }

                for batch in &decoded_batches {
                    println!(
                        "[commands] batch {} entries {}",
                        batch.sequence,
                        batch.entries.len()
                    );
                }

                if let Some(entity) = self.command_entity {
                    let mut packets_to_queue: Vec<CommandPacket> = Vec::new();

                    if !decoded_batches.is_empty() {
                        let mut outbox_packets = None;
                        {
                            let world = self.scheduler.world_mut();
                            if let Some(outbox) = world.get_mut::<CommandOutbox>(entity) {
                                outbox.ingest(decoded_batches);
                                outbox_packets = Some(outbox.drain_packets());
                            }
                        }

                        if let Some(mut drained) = outbox_packets
                            && !drained.is_empty()
                        {
                            packets_to_queue.append(&mut drained);
                        }
                    }

                    // If no packets were drained from the outbox, fall back to the original packets.
                    // This ensures that any command packets not processed by the outbox are still queued for transport,
                    // preventing loss of commands in cases where the outbox is empty or not used.
                    if packets_to_queue.is_empty() {
                        packets_to_queue = packets.clone();
                    }

                    if !packets_to_queue.is_empty() {
                        #[cfg(feature = "network-quic")]
                        let mut pending_dispatch: Option<
                            Vec<CommandPacket>,
                        > = None;

                        {
                            let world = self.scheduler.world_mut();
                            if let Some(queue) = world.get_mut::<CommandTransportQueue>(entity) {
                                queue.enqueue(packets_to_queue.iter().cloned());
                                #[cfg(feature = "network-quic")]
                                if self.command_transport.is_some() {
                                    let drained = queue.drain_pending();
                                    if !drained.is_empty() {
                                        pending_dispatch = Some(drained);
                                    }
                                }
                            }
                        }

                        for packet in &packets_to_queue {
                            log::info!(
                                "[commands] transport queued seq {} ({} bytes)",
                                packet.sequence,
                                packet.payload.len()
                            );
                        }

                        #[cfg(feature = "network-quic")]
                        if let Some(packets_to_send) = pending_dispatch {
                            if let Some(transport) = self.command_transport.as_ref() {
                                let send_result = {
                                    let runtime = self.network_runtime.get_or_insert_with(|| {
                                        TokioRuntimeBuilder::new_current_thread()
                                            .enable_all()
                                            .build()
                                            .expect("create network runtime")
                                    });
                                    runtime
                                        .block_on(transport.send_command_packets(&packets_to_send))
                                };

                                if let Err(err) = send_result {
                                    log::error!(
                                        "[commands] failed to transmit {} packets: {err}",
                                        packets_to_send.len()
                                    );
                                    let world = self.scheduler.world_mut();
                                    if let Some(queue) =
                                        world.get_mut::<CommandTransportQueue>(entity)
                                    {
                                        queue.enqueue(packets_to_send);
                                    }
                                } else {
                                    log::debug!(
                                        "[commands] transmitted {} queued packets via {:?}",
                                        packets_to_send.len(),
                                        transport.kind()
                                    );
                                }
                            } else {
                                let world = self.scheduler.world_mut();
                                if let Some(queue) = world.get_mut::<CommandTransportQueue>(entity)
                                {
                                    queue.enqueue(packets_to_send);
                                }
                            }
                        }
                    }
                }
            }

            if let Some(entity) = self.command_entity
                && let Some(depth) = self
                    .scheduler
                    .world()
                    .get::<CommandTransportQueue>(entity)
                    .map(|queue| queue.pending_depth())
            {
                pipeline.update_queue_depth(depth);
            }

            command_metrics_snapshot = Some(pipeline.metrics_snapshot());
        }

        if let Some(sample) = telemetry_sample.as_mut() {
            sample.set_command_metrics(command_metrics_snapshot.clone());
        }

        if let (Some(entity), Some(sample)) = (self.telemetry_entity, telemetry_sample) {
            let world = self.scheduler.world_mut();
            let mut latest_to_publish = None;

            if let Some(surface) = world.get_mut::<TelemetrySurface>(entity)
                && surface.record(sample)
            {
                latest_to_publish = surface.latest().cloned();
            }

            if let Some(mut latest) = latest_to_publish {
                if command_metrics_snapshot.is_some() {
                    latest.set_command_metrics(command_metrics_snapshot.clone());
                }
                if let Some(replicator) = world.get_mut::<TelemetryReplicator>(entity) {
                    replicator.publish(entity, &latest);
                }
            }
        }
    }

    #[cfg(feature = "network-quic")]
    fn poll_remote_commands(&mut self) {
        loop {
            let packet = match self.receive_next_command_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(err) => {
                    log::error!("[transport] failed to receive command packet: {err}");
                    break;
                }
            };

            let applied_entries = match self.command_pipeline.lock() {
                Ok(mut pipeline) => match pipeline.integrate_remote_packet(&packet) {
                    Ok(entries) => entries,
                    Err(err) => {
                        log::error!(
                            "[commands] failed to integrate remote packet {}: {err}",
                            packet.sequence
                        );
                        continue;
                    }
                },
                Err(err) => {
                    log::error!(
                        "[commands] command pipeline mutex poisoned while integrating remote packet: {err}"
                    );
                    break;
                }
            };

            if applied_entries.is_empty() {
                continue;
            }

            self.apply_remote_entries(&applied_entries);
        }
    }

    #[cfg(feature = "network-quic")]
    fn receive_next_command_packet(&mut self) -> Result<Option<CommandPacket>, TransportError> {
        let transport = match self.command_transport.as_ref() {
            Some(transport) => transport,
            None => return Ok(None),
        };

        let runtime = self.network_runtime.get_or_insert_with(|| {
            TokioRuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .expect("create network runtime")
        });

        runtime.block_on(transport.receive_command_packet(Duration::from_millis(0)))
    }

    #[cfg_attr(not(any(feature = "network-quic", test)), allow(dead_code))]
    fn apply_remote_entries(&mut self, entries: &[CommandEntry]) {
        if entries.is_empty() {
            return;
        }

        let Some(editor_entity) = self.command_entity else {
            log::warn!("[commands] editor entity unavailable; skipping remote command apply");
            return;
        };

        let world = self.scheduler.world_mut();
        for entry in entries {
            match entry.payload.command_type.as_str() {
                CMD_SELECTION_HIGHLIGHT => {
                    if !matches!(entry.payload.scope, CommandScope::Entity(_)) {
                        log::warn!(
                            "[commands] selection highlight command missing entity scope (id {id:?})",
                            id = entry.id
                        );
                        continue;
                    }

                    match serde_json::from_slice::<SelectionHighlightCommand>(&entry.payload.data) {
                        Ok(command) => {
                            let target_entity = crate::ecs::Entity::from(command.entity);
                            let exists = world.contains(target_entity);

                            match world.get_mut::<EditorSelection>(editor_entity) {
                                Some(selection) => {
                                    if exists {
                                        selection.primary = Some(target_entity);
                                    } else {
                                        log::warn!(
                                            "[commands] remote highlight target {entity:?} missing locally",
                                            entity = command.entity
                                        );
                                    }
                                    selection.highlight_active = command.active;
                                    selection.frames_since_change = 0;
                                }
                                None => {
                                    log::warn!(
                                        "[commands] editor selection component missing on {editor_entity:?}"
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode SelectionHighlightCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_ENTITY_TRANSLATE => {
                    match serde_json::from_slice::<EntityTranslateCommand>(&entry.payload.data) {
                        Ok(command) => {
                            let target_entity = crate::ecs::Entity::from(command.entity);
                            if let Some(transform) = world.get_mut::<Transform>(target_entity) {
                                for (axis, delta) in
                                    transform.position.iter_mut().zip(command.delta.iter())
                                {
                                    *axis += *delta;
                                }
                            } else {
                                log::warn!(
                                    "[commands] translate target {entity:?} missing transform",
                                    entity = command.entity
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode EntityTranslateCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_ENTITY_ROTATE => {
                    match serde_json::from_slice::<EntityRotateCommand>(&entry.payload.data) {
                        Ok(command) => {
                            let target_entity = crate::ecs::Entity::from(command.entity);
                            if let Some(transform) = world.get_mut::<Transform>(target_entity) {
                                transform.rotation = [
                                    command.rotation.x,
                                    command.rotation.y,
                                    command.rotation.z,
                                    command.rotation.w,
                                ];
                            } else {
                                log::warn!(
                                    "[commands] rotate target {entity:?} missing transform",
                                    entity = command.entity
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode EntityRotateCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_ENTITY_SCALE => {
                    match serde_json::from_slice::<EntityScaleCommand>(&entry.payload.data) {
                        Ok(command) => {
                            let target_entity = crate::ecs::Entity::from(command.entity);
                            if let Some(transform) = world.get_mut::<Transform>(target_entity) {
                                transform.scale = sanitize_scale(command.scale);
                            } else {
                                log::warn!(
                                    "[commands] scale target {entity:?} missing transform",
                                    entity = command.entity
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode EntityScaleCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_TOOL_ACTIVATE => {
                    match serde_json::from_slice::<ToolActivateCommand>(&entry.payload.data) {
                        Ok(command) => {
                            if let Some(tool_state) =
                                world.get_mut::<EditorToolState>(editor_entity)
                            {
                                tool_state.activate(command.tool_id.clone(), entry.id.lamport());
                            } else {
                                log::warn!(
                                    "[commands] editor tool state component missing on {editor_entity:?}"
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode ToolActivateCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_TOOL_DEACTIVATE => {
                    match serde_json::from_slice::<ToolDeactivateCommand>(&entry.payload.data) {
                        Ok(command) => {
                            if let Some(tool_state) =
                                world.get_mut::<EditorToolState>(editor_entity)
                            {
                                if tool_state.matches_active(&command.tool_id) {
                                    tool_state.deactivate(entry.id.lamport());
                                }
                            } else {
                                log::warn!(
                                    "[commands] editor tool state component missing on {editor_entity:?}"
                                );
                            }
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode ToolDeactivateCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_MESH_VERTEX_CREATE => {
                    match serde_json::from_slice::<VertexCreateCommand>(&entry.payload.data) {
                        Ok(command) => {
                            log::debug!(
                                "[commands] mesh vertex create queued at position {:?} metadata {:?}",
                                command.position,
                                command.metadata
                            );
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode VertexCreateCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_MESH_EDGE_EXTRUDE => {
                    match serde_json::from_slice::<EdgeExtrudeCommand>(&entry.payload.data) {
                        Ok(command) => {
                            log::debug!(
                                "[commands] mesh edge {} extrude direction {:?}",
                                command.edge_id,
                                command.direction
                            );
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode EdgeExtrudeCommand payload: {err}"
                            );
                        }
                    }
                }
                CMD_MESH_FACE_SUBDIVIDE => {
                    match serde_json::from_slice::<FaceSubdivideCommand>(&entry.payload.data) {
                        Ok(command) => {
                            log::debug!(
                                "[commands] mesh face {} subdivide levels {} smoothness {:.3}",
                                command.face_id,
                                command.params.levels,
                                command.params.smoothness
                            );
                        }
                        Err(err) => {
                            log::error!(
                                "[commands] failed to decode FaceSubdivideCommand payload: {err}"
                            );
                        }
                    }
                }
                other => {
                    log::debug!("[commands] ignoring unhandled remote command type {other}");
                }
            }
        }
    }
}

crate::register_component_types!(
    FrameStats,
    Transform,
    Velocity,
    EditorSelection,
    EditorToolState,
    CommandOutbox,
    CommandTransportQueue
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::{
        EntityRotateCommand, EntityScaleCommand, EntityTranslateCommand, Quaternion,
        ToolActivateCommand, ToolDeactivateCommand,
    };
    use crate::network::command_log::{
        AuthorId, CommandAuthor, CommandEntry, CommandId, CommandPayload, CommandRole,
        CommandScope, ConflictStrategy,
    };

    #[test]
    fn apply_remote_selection_highlight_updates_world() {
        let mut engine = Engine::new();

        let (editor_entity, primary_entity) = {
            let world = engine.world();
            let mut entries = world.component_entries::<EditorSelection>();
            assert_eq!(entries.len(), 1);
            let (entity, selection) = entries.remove(0);
            (
                entity,
                selection.primary.expect("selection should have primary"),
            )
        };

        let handle = EntityHandle::from(primary_entity);
        let command = SelectionHighlightCommand::new(handle, false);
        let payload = serde_json::to_vec(&command).expect("serialize selection command");
        let entry = CommandEntry::new(
            CommandId::new(5, AuthorId(3)),
            123,
            CommandPayload::new(
                CMD_SELECTION_HIGHLIGHT,
                CommandScope::Entity(handle),
                payload,
            ),
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(3), CommandRole::Editor),
            None,
        );

        engine.apply_remote_entries(&[entry]);

        let world = engine.world();
        let selection = world
            .get::<EditorSelection>(editor_entity)
            .expect("editor selection present");
        assert_eq!(selection.primary, Some(primary_entity));
        assert!(!selection.highlight_active);
        assert_eq!(selection.frames_since_change, 0);
    }

    #[test]
    fn transform_commands_mutate_entities() {
        let mut engine = Engine::new();

        let (primary_entity, handle) = {
            let world = engine.world();
            let selection_entry = world
                .component_entries::<EditorSelection>()
                .into_iter()
                .next()
                .expect("selection component present");
            let primary = selection_entry
                .1
                .primary
                .expect("selection should have primary");
            let handle = EntityHandle::from(primary);
            (primary, handle)
        };

        let mut original_position = [0.0f32; 3];
        if let Some(transform) = engine.world().get::<Transform>(primary_entity) {
            original_position = transform.position;
        }

        let translate = EntityTranslateCommand::new(handle, [0.5, -0.25, 0.0]);
        let translate_entry = CommandEntry::new(
            CommandId::new(10, AuthorId(1)),
            1,
            CommandPayload::new(
                CMD_ENTITY_TRANSLATE,
                CommandScope::Entity(handle),
                serde_json::to_vec(&translate).unwrap(),
            ),
            ConflictStrategy::Merge,
            CommandAuthor::new(AuthorId(1), CommandRole::Editor),
            None,
        );
        engine.apply_remote_entries(&[translate_entry]);

        let mutated = engine
            .world()
            .get::<Transform>(primary_entity)
            .expect("transform present");
        assert!((mutated.position[0] - (original_position[0] + 0.5)).abs() < 1e-5);
        assert!((mutated.position[1] - (original_position[1] - 0.25)).abs() < 1e-5);

        let rotate = EntityRotateCommand::new(
            handle,
            Quaternion::new(0.0, 0.0, 0.707_106_77, 0.707_106_77),
        );
        let rotate_entry = CommandEntry::new(
            CommandId::new(11, AuthorId(1)),
            2,
            CommandPayload::new(
                CMD_ENTITY_ROTATE,
                CommandScope::Entity(handle),
                serde_json::to_vec(&rotate).unwrap(),
            ),
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(1), CommandRole::Editor),
            None,
        );
        engine.apply_remote_entries(&[rotate_entry]);

        let rotated = engine
            .world()
            .get::<Transform>(primary_entity)
            .expect("transform present");
        assert!((rotated.rotation[3] - 0.707_106_77).abs() < 1e-5);

        let scale = EntityScaleCommand::new(handle, [2.0, 1.0, 0.5]);
        let scale_entry = CommandEntry::new(
            CommandId::new(12, AuthorId(1)),
            3,
            CommandPayload::new(
                CMD_ENTITY_SCALE,
                CommandScope::Entity(handle),
                serde_json::to_vec(&scale).unwrap(),
            ),
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(1), CommandRole::Editor),
            None,
        );
        engine.apply_remote_entries(&[scale_entry]);

        let scaled = engine
            .world()
            .get::<Transform>(primary_entity)
            .expect("transform present");
        assert!((scaled.scale[0] - 2.0).abs() < 1e-5);
        assert!((scaled.scale[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn tool_state_commands_track_active_tool() {
        let mut engine = Engine::new();

        let editor_entity = {
            let world = engine.world();
            let entry = world
                .component_entries::<EditorSelection>()
                .into_iter()
                .next()
                .expect("editor selection present");
            entry.0
        };

        let activate = ToolActivateCommand::new("gizmo.translate");
        let activate_entry = CommandEntry::new(
            CommandId::new(20, AuthorId(2)),
            10,
            CommandPayload::new(
                CMD_TOOL_ACTIVATE,
                CommandScope::Tool("gizmo.translate".into()),
                serde_json::to_vec(&activate).unwrap(),
            ),
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(2), CommandRole::Editor),
            None,
        );
        engine.apply_remote_entries(&[activate_entry]);

        {
            let world = engine.world();
            let tool_state = world
                .get::<EditorToolState>(editor_entity)
                .expect("tool state present");
            assert_eq!(tool_state.active_tool.as_deref(), Some("gizmo.translate"));
            assert_eq!(tool_state.last_lamport, Some(20));
        }

        let deactivate = ToolDeactivateCommand::new("gizmo.translate");
        let deactivate_entry = CommandEntry::new(
            CommandId::new(21, AuthorId(2)),
            11,
            CommandPayload::new(
                CMD_TOOL_DEACTIVATE,
                CommandScope::Tool("gizmo.translate".into()),
                serde_json::to_vec(&deactivate).unwrap(),
            ),
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(2), CommandRole::Editor),
            None,
        );
        engine.apply_remote_entries(&[deactivate_entry]);

        let world = engine.world();
        let tool_state = world
            .get::<EditorToolState>(editor_entity)
            .expect("tool state present");
        assert!(tool_state.active_tool.is_none());
        assert_eq!(tool_state.last_lamport, Some(21));
    }
}
