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
use crate::editor::telemetry::{
    FrameTelemetry, TelemetryReplicator, TelemetrySurface, WebRtcTelemetry,
};
#[cfg(feature = "network-quic")]
use crate::editor::telemetry::{WebRtcIceMetrics, WebRtcLinkMetrics, WebRtcPeerSample};
use crate::editor::{CommandOutbox, CommandTransportQueue};
use crate::network::EntityHandle;
use crate::network::command_log::{CommandBatch, CommandEntry, CommandPacket, CommandScope};
#[cfg(feature = "network-quic")]
use crate::network::signaling::{
    IceCandidate, PeerId, RoomId, SessionDescription, SignalingClient, SignalingError,
    SignalingHandle, SignalingResponse, SignalingServer,
};
#[cfg(feature = "network-quic")]
use crate::network::transport::{
    CommandTransport, TransportError, TransportSession, WebRtcTransport,
};
#[cfg(feature = "network-quic")]
use crate::network::{TransportDiagnostics, TransportKind};
use crate::render::{BackendKind, GpuBackend, NullGpuBackend, Renderer, RendererConfig};
#[cfg(feature = "vr-openxr")]
use crate::vr::openxr::OpenXrInputProvider;
use crate::vr::{
    ControllerState, NullVrBridge, SimulatedInputProvider, TrackedPose, VrBridge, VrInputProvider,
};
use schedule::{Scheduler, Stage, System};
use serde::{Deserialize, Serialize};
#[cfg(feature = "network-quic")]
use std::collections::{HashMap, HashSet};
#[cfg(feature = "network-quic")]
use std::env;
#[cfg(feature = "network-quic")]
use std::net::{IpAddr, SocketAddr};
#[cfg(feature = "network-quic")]
use std::process;
#[cfg(feature = "network-quic")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "network-quic")]
use std::time::Duration;
use std::time::Instant;
#[cfg(feature = "network-quic")]
use tokio::runtime::{Builder as TokioRuntimeBuilder, Runtime as TokioRuntime};
#[cfg(feature = "network-quic")]
use tokio::sync::Mutex as TokioMutex;
#[cfg(feature = "network-quic")]
use tokio::sync::mpsc::error::TryRecvError;
#[cfg(feature = "network-quic")]
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
#[cfg(feature = "network-quic")]
use url::Url;
#[cfg(feature = "network-quic")]
use webrtc::api::API;
#[cfg(feature = "network-quic")]
use webrtc::api::APIBuilder;
#[cfg(feature = "network-quic")]
use webrtc::api::interceptor_registry::register_default_interceptors;
#[cfg(feature = "network-quic")]
use webrtc::api::media_engine::MediaEngine;
#[cfg(feature = "network-quic")]
use webrtc::data_channel::RTCDataChannel;
#[cfg(feature = "network-quic")]
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
#[cfg(feature = "network-quic")]
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
#[cfg(feature = "network-quic")]
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
#[cfg(feature = "network-quic")]
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
#[cfg(feature = "network-quic")]
use webrtc::ice_transport::ice_credential_type::RTCIceCredentialType;
#[cfg(feature = "network-quic")]
use webrtc::ice_transport::ice_server::RTCIceServer;
#[cfg(feature = "network-quic")]
use webrtc::interceptor::registry::Registry;
#[cfg(feature = "network-quic")]
use webrtc::peer_connection::RTCPeerConnection;
#[cfg(feature = "network-quic")]
use webrtc::peer_connection::configuration::RTCConfiguration;
#[cfg(feature = "network-quic")]
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
#[cfg(feature = "network-quic")]
use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;
#[cfg(feature = "network-quic")]
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

const DEFAULT_MAX_FRAMES: u32 = 3;

#[cfg(feature = "network-quic")]
const WEBRTC_OFFER_RETRY_MAX: u32 = 3;
#[cfg(feature = "network-quic")]
const WEBRTC_OFFER_RETRY_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(feature = "network-quic")]
const WEBRTC_ICE_RETRY_INTERVAL: Duration = Duration::from_secs(3);
#[cfg(feature = "network-quic")]
const WEBRTC_NEGOTIATION_STALE_AFTER: Duration = Duration::from_secs(30);
#[cfg(feature = "network-quic")]
const WEBRTC_ICE_VALIDATION_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(feature = "network-quic")]
const WEBRTC_RECONNECT_DELAY: Duration = Duration::from_secs(5);

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
    command_transport_fallback: Option<CommandTransport>,
    #[cfg(feature = "network-quic")]
    network_runtime: Option<TokioRuntime>,
    #[cfg(feature = "network-quic")]
    signaling_handle: Option<SignalingHandle>,
    #[cfg(feature = "network-quic")]
    signaling_clients: HashMap<PeerId, Arc<TokioMutex<SignalingClient>>>,
    #[cfg(feature = "network-quic")]
    local_signaling_peer: Option<PeerId>,
    #[cfg(feature = "network-quic")]
    signaling_room: Option<RoomId>,
    #[cfg(feature = "network-quic")]
    signaling_endpoint: Option<Url>,
    #[cfg(feature = "network-quic")]
    signaling_events_polled: u64,
    #[cfg(feature = "network-quic")]
    webrtc_peers: HashMap<PeerId, WebRtcPeerEntry>,
    #[cfg(feature = "network-quic")]
    webrtc_event_tx: UnboundedSender<WebRtcRuntimeEvent>,
    #[cfg(feature = "network-quic")]
    webrtc_event_rx: UnboundedReceiver<WebRtcRuntimeEvent>,
    #[cfg(feature = "network-quic")]
    active_webrtc_peer: Option<PeerId>,
    #[cfg(feature = "network-quic")]
    webrtc_ice_servers: Vec<IceServerConfig>,
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
        #[cfg(feature = "network-quic")]
        let (webrtc_event_tx, webrtc_event_rx) = unbounded_channel();

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
            command_transport_fallback: None,
            #[cfg(feature = "network-quic")]
            network_runtime: None,
            #[cfg(feature = "network-quic")]
            signaling_handle: None,
            #[cfg(feature = "network-quic")]
            signaling_clients: HashMap::new(),
            #[cfg(feature = "network-quic")]
            local_signaling_peer: None,
            #[cfg(feature = "network-quic")]
            signaling_room: None,
            #[cfg(feature = "network-quic")]
            signaling_endpoint: None,
            #[cfg(feature = "network-quic")]
            signaling_events_polled: 0,
            #[cfg(feature = "network-quic")]
            webrtc_peers: HashMap::new(),
            #[cfg(feature = "network-quic")]
            webrtc_event_tx,
            #[cfg(feature = "network-quic")]
            webrtc_event_rx,
            #[cfg(feature = "network-quic")]
            active_webrtc_peer: None,
            #[cfg(feature = "network-quic")]
            webrtc_ice_servers: load_webrtc_ice_servers(),
        };

        engine.register_core_systems();
        #[cfg(feature = "network-quic")]
        if let Err(err) = engine.bootstrap_signaling() {
            log::error!("[engine] failed to bootstrap signaling: {err}");
        }
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

        self.ensure_network_runtime();
        self.command_transport = Some(transport);
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_transport_session(&mut self, session: TransportSession) {
        self.attach_command_transport(CommandTransport::from(session));
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_webrtc_transport(&mut self, transport: WebRtcTransport) {
        self.attach_command_transport(CommandTransport::from(transport));
    }

    #[cfg(feature = "network-quic")]
    pub fn start_signaling_server(
        &mut self,
        addr: SocketAddr,
    ) -> Result<SocketAddr, SignalingError> {
        self.ensure_network_runtime();

        if let Some(handle) = self.signaling_handle.take() {
            self.shutdown_signaling_handle(handle);
            self.signaling_endpoint = None;
        }

        self.signaling_clients.clear();
        self.local_signaling_peer = None;
        self.signaling_room = None;

        let server = {
            let runtime = self
                .network_runtime
                .as_mut()
                .expect("network runtime should exist");
            runtime.block_on(SignalingServer::bind(addr))?
        };
        let local_addr = server.local_addr();
        let handle = {
            let runtime = self
                .network_runtime
                .as_mut()
                .expect("network runtime should exist");
            runtime.block_on(async move { server.start() })
        };
        self.signaling_handle = Some(handle);
        Ok(local_addr)
    }

    #[cfg(feature = "network-quic")]
    pub fn active_signaling_addr(&self) -> Option<SocketAddr> {
        self.signaling_handle
            .as_ref()
            .map(|handle| handle.local_addr())
    }

    #[cfg(feature = "network-quic")]
    pub fn stop_signaling_server(&mut self) {
        let had_handle = self.signaling_handle.is_some();
        if let Some(handle) = self.signaling_handle.take() {
            self.shutdown_signaling_handle(handle);
        }

        if had_handle {
            self.signaling_endpoint = None;
        }

        self.signaling_clients.clear();
        self.local_signaling_peer = None;
        self.signaling_room = None;
        self.webrtc_peers.clear();
    }

    #[cfg(feature = "network-quic")]
    pub fn drain_signaling_errors(&mut self) -> Vec<String> {
        if let Some(handle) = self.signaling_handle.as_ref() {
            return handle.drain_errors();
        }

        Vec::new()
    }

    #[cfg(feature = "network-quic")]
    pub fn connect_signaling_client(
        &mut self,
        endpoint: Url,
        peer_id: PeerId,
        room_id: RoomId,
        timeout: Duration,
    ) -> Result<Vec<PeerId>, SignalingError> {
        self.ensure_network_runtime();
        let mut client = {
            let runtime = self
                .network_runtime
                .as_mut()
                .expect("network runtime should exist");
            runtime.block_on(SignalingClient::connect(endpoint, peer_id.clone(), room_id))?
        };
        let peers = {
            let runtime = self
                .network_runtime
                .as_mut()
                .expect("network runtime should exist");
            runtime.block_on(client.register(timeout))?
        };
        let client = Arc::new(TokioMutex::new(client));
        self.signaling_clients.insert(peer_id, client);
        Ok(peers)
    }

    #[cfg(feature = "network-quic")]
    pub fn signaling_client(&self, peer_id: &PeerId) -> Option<Arc<TokioMutex<SignalingClient>>> {
        self.signaling_clients.get(peer_id).cloned()
    }

    #[cfg(feature = "network-quic")]
    pub fn remove_signaling_client(&mut self, peer_id: &PeerId) {
        self.signaling_clients.remove(peer_id);
    }

    #[cfg(feature = "network-quic")]
    pub fn signaling_next_event(
        &mut self,
        peer_id: &PeerId,
        timeout: Duration,
    ) -> Result<Option<SignalingResponse>, SignalingError> {
        let client = match self.signaling_clients.get(peer_id) {
            Some(client) => Arc::clone(client),
            None => return Ok(None),
        };

        let runtime = self.ensure_network_runtime();
        runtime.block_on(async move {
            let mut guard = client.lock().await;
            guard.next_event(timeout).await
        })
    }

    #[cfg(feature = "network-quic")]
    pub fn signaling_send_offer(
        &mut self,
        from: &PeerId,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        let client = self
            .signaling_clients
            .get(from)
            .cloned()
            .ok_or(SignalingError::ClientNotRegistered)?;
        let to_peer = to.clone();
        let runtime = self.ensure_network_runtime();
        runtime.block_on(async move {
            let mut guard = client.lock().await;
            guard.send_offer(&to_peer, sdp).await
        })
    }

    #[cfg(feature = "network-quic")]
    pub fn signaling_send_answer(
        &mut self,
        from: &PeerId,
        to: &PeerId,
        sdp: SessionDescription,
    ) -> Result<(), SignalingError> {
        let client = self
            .signaling_clients
            .get(from)
            .cloned()
            .ok_or(SignalingError::ClientNotRegistered)?;
        let to_peer = to.clone();
        let runtime = self.ensure_network_runtime();
        runtime.block_on(async move {
            let mut guard = client.lock().await;
            guard.send_answer(&to_peer, sdp).await
        })
    }

    #[cfg(feature = "network-quic")]
    pub fn signaling_send_ice_candidate(
        &mut self,
        from: &PeerId,
        to: &PeerId,
        candidate: IceCandidate,
    ) -> Result<(), SignalingError> {
        let client = self
            .signaling_clients
            .get(from)
            .cloned()
            .ok_or(SignalingError::ClientNotRegistered)?;
        let to_peer = to.clone();
        let runtime = self.ensure_network_runtime();
        runtime.block_on(async move {
            let mut guard = client.lock().await;
            guard.send_ice_candidate(&to_peer, candidate).await
        })
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

#[cfg(feature = "network-quic")]
impl Engine {
    fn ensure_network_runtime(&mut self) -> &mut TokioRuntime {
        if self.network_runtime.is_none() {
            self.network_runtime = Some(
                TokioRuntimeBuilder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("create network runtime"),
            );
        }

        self.network_runtime
            .as_mut()
            .expect("network runtime should exist")
    }

    fn build_rtc_configuration(&self) -> RTCConfiguration {
        let mut config = RTCConfiguration::default();
        if !self.webrtc_ice_servers.is_empty() {
            config.ice_servers = self
                .webrtc_ice_servers
                .iter()
                .map(IceServerConfig::to_rtc)
                .collect();
        }
        config
    }

    fn ensure_peer_connection(
        &mut self,
        peer_id: &PeerId,
    ) -> Result<Arc<RTCPeerConnection>, String> {
        if let Some(existing) = self
            .webrtc_peers
            .get(peer_id)
            .and_then(|entry| entry.connection.as_ref().cloned())
        {
            return Ok(existing);
        }

        let api = build_webrtc_api()?;
        let config = self.build_rtc_configuration();
        let runtime = self.ensure_network_runtime();
        let connection = runtime
            .block_on(async { api.new_peer_connection(config).await })
            .map_err(|err| err.to_string())?;
        let connection = Arc::new(connection);
        self.configure_peer_connection_callbacks(peer_id.clone(), &connection)?;
        self.ensure_webrtc_entry(peer_id).connection = Some(Arc::clone(&connection));
        Ok(connection)
    }

    fn install_active_transport(&mut self, transport: CommandTransport) -> TransportKind {
        let kind = transport.kind();
        let metrics = transport.metrics_handle();
        if let Ok(mut pipeline) = self.command_pipeline.lock() {
            pipeline.attach_transport_metrics(metrics);
        }
        self.command_transport = Some(transport);
        kind
    }

    fn stash_current_transport_for_fallback(&mut self) {
        if let Some(existing) = self.command_transport.take() {
            match existing {
                CommandTransport::WebRtc(_) => {
                    self.close_transport(existing);
                }
                other => {
                    if let Some(replaced) = self.command_transport_fallback.replace(other) {
                        self.close_transport(replaced);
                    }
                }
            }
        }
    }

    fn close_transport(&mut self, transport: CommandTransport) {
        let runtime = self.ensure_network_runtime();
        runtime.block_on(async move {
            transport.close().await;
        });
    }

    fn reactivate_fallback_transport(&mut self) {
        if let Some(fallback) = self.command_transport_fallback.take() {
            let kind = self.install_active_transport(fallback);
            log::info!("[webrtc] restored fallback {kind:?} command transport after WebRTC drop");
        }
    }

    fn shutdown_active_webrtc_transport(&mut self) {
        let mut needs_fallback = false;

        if let Some(active) = self.command_transport.take() {
            match active {
                CommandTransport::WebRtc(_) => {
                    self.close_transport(active);
                    needs_fallback = true;
                }
                other => {
                    self.command_transport = Some(other);
                }
            }
        } else {
            needs_fallback = true;
        }

        if needs_fallback {
            self.active_webrtc_peer = None;
            self.reactivate_fallback_transport();
        }
    }

    fn configure_peer_connection_callbacks(
        &mut self,
        peer_id: PeerId,
        connection: &Arc<RTCPeerConnection>,
    ) -> Result<(), String> {
        let Some(local_peer) = self.local_signaling_peer.clone() else {
            return Err("local signaling peer unavailable".into());
        };
        let Some(signaling_client) = self.signaling_client(&local_peer) else {
            return Err("signaling client not registered".into());
        };

        let event_tx = self.webrtc_event_tx.clone();
        let remote_for_state = peer_id.clone();
        connection.on_peer_connection_state_change(Box::new(move |state| {
            let tx = event_tx.clone();
            let peer = remote_for_state.clone();
            Box::pin(async move {
                let _ = tx.send(WebRtcRuntimeEvent::ConnectionState {
                    peer_id: peer,
                    state,
                });
            })
        }));

        let signaling_for_ice = Arc::clone(&signaling_client);
        let remote_for_ice = peer_id.clone();
        let event_tx_for_ice = self.webrtc_event_tx.clone();
        connection.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let signaling = Arc::clone(&signaling_for_ice);
            let remote = remote_for_ice.clone();
            let event_tx = event_tx_for_ice.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else {
                    return;
                };
                match candidate.to_json() {
                    Ok(json) => {
                        let candidate = IceCandidate {
                            candidate: json.candidate,
                            sdp_mid: json.sdp_mid,
                            sdp_mline_index: json.sdp_mline_index,
                        };
                        let mut client = signaling.lock().await;
                        let send_result = client.send_ice_candidate(&remote, candidate.clone()).await;
                        drop(client);
                        if let Err(err) = send_result {
                            log::warn!(
                                "[webrtc] failed to send ICE candidate to {}: {err}",
                                remote.0
                            );
                        } else if event_tx
                            .send(WebRtcRuntimeEvent::LocalIceCandidate {
                                peer_id: remote.clone(),
                                candidate,
                            })
                            .is_err()
                        {
                            log::trace!(
                                "[webrtc] dropping local ICE candidate event for {}; receiver closed",
                                remote.0
                            );
                        }
                    }
                    Err(err) => {
                        log::warn!("[webrtc] failed to serialize ICE candidate: {err}");
                    }
                }
            })
        }));

        let event_tx_for_channel = self.webrtc_event_tx.clone();
        let connection_for_channel = Arc::clone(connection);
        let peer_for_channel = peer_id.clone();
        connection.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
            let event_tx = event_tx_for_channel.clone();
            let peer = peer_for_channel.clone();
            let connection = Arc::clone(&connection_for_channel);
            Box::pin(async move {
                hook_transport_emitter(&event_tx, peer, connection, channel);
            })
        }));

        Ok(())
    }

    fn create_local_offer(&mut self, peer_id: &PeerId) -> Result<SessionDescription, String> {
        let connection = self.ensure_peer_connection(peer_id)?;
        let needs_data_channel = !self
            .webrtc_peers
            .get(peer_id)
            .map(|entry| entry.has_local_data_channel)
            .unwrap_or(false);

        if needs_data_channel {
            let runtime = self.ensure_network_runtime();
            let channel = runtime
                .block_on(async {
                    connection
                        .create_data_channel(
                            "theta-command",
                            Some(RTCDataChannelInit {
                                ordered: Some(true),
                                ..Default::default()
                            }),
                        )
                        .await
                })
                .map_err(|err| err.to_string())?;
            hook_transport_emitter(
                &self.webrtc_event_tx,
                peer_id.clone(),
                Arc::clone(&connection),
                channel,
            );
            if let Some(entry) = self.webrtc_peers.get_mut(peer_id) {
                entry.has_local_data_channel = true;
            }
        }

        let runtime = self.ensure_network_runtime();
        let local_description = runtime.block_on(async {
            let offer = connection
                .create_offer(None)
                .await
                .map_err(|err| err.to_string())?;
            connection
                .set_local_description(offer)
                .await
                .map_err(|err| err.to_string())?;
            connection
                .local_description()
                .await
                .ok_or_else(|| "missing local description after offer".to_string())
        })?;

        Ok(session_description_from_rtc(&local_description))
    }

    fn accept_remote_offer(
        &mut self,
        peer_id: &PeerId,
        remote: SessionDescription,
    ) -> Result<(), String> {
        let connection = self.ensure_peer_connection(peer_id)?;
        let runtime = self.ensure_network_runtime();
        let remote_desc = rtc_session_from(&remote)?;
        runtime.block_on(async {
            connection
                .set_remote_description(remote_desc)
                .await
                .map_err(|err| err.to_string())
        })?;

        let answer = runtime.block_on(async {
            let answer = connection
                .create_answer(None)
                .await
                .map_err(|err| err.to_string())?;
            connection
                .set_local_description(answer)
                .await
                .map_err(|err| err.to_string())?;
            connection
                .local_description()
                .await
                .ok_or_else(|| "missing local description after answer".to_string())
        })?;

        let local_peer = self
            .local_signaling_peer
            .clone()
            .ok_or_else(|| "local signaling peer unavailable".to_string())?;
        let session = session_description_from_rtc(&answer);
        self.signaling_send_answer(&local_peer, peer_id, session)
            .map_err(|err| err.to_string())?;

        if let Some(entry) = self.webrtc_peers.get_mut(peer_id) {
            entry.pending_remote_sdp = None;
            entry.state = WebRtcConnectionPhase::AwaitingIceCompletion;
            entry.last_event = Instant::now();
        }

        self.flush_pending_ice(peer_id);
        Ok(())
    }

    fn apply_remote_answer(
        &mut self,
        peer_id: &PeerId,
        answer: SessionDescription,
    ) -> Result<(), String> {
        {
            let Some(entry) = self.webrtc_peers.get(peer_id) else {
                return Err(format!(
                    "peer {} missing during answer application",
                    peer_id.0
                ));
            };
            if entry.connection.is_none() {
                return Err(format!(
                    "no peer connection established for {} while applying answer",
                    peer_id.0
                ));
            }
        }

        let connection = self.ensure_peer_connection(peer_id)?;
        let runtime = self.ensure_network_runtime();
        let remote_desc = rtc_session_from(&answer)?;
        runtime.block_on(async {
            connection
                .set_remote_description(remote_desc)
                .await
                .map_err(|err| err.to_string())
        })?;

        if let Some(entry) = self.webrtc_peers.get_mut(peer_id) {
            entry.pending_remote_sdp = None;
            entry.state = WebRtcConnectionPhase::AwaitingIceCompletion;
            entry.last_event = Instant::now();
        }

        self.flush_pending_ice(peer_id);
        Ok(())
    }

    fn flush_pending_ice(&mut self, peer_id: &PeerId) {
        let (connection, mut pending) = {
            let now = Instant::now();
            let Some(entry) = self.webrtc_peers.get_mut(peer_id) else {
                return;
            };
            if entry.pending_ice.is_empty() {
                return;
            }

            if let Some(last_retry) = entry.last_ice_retry
                && now.duration_since(last_retry) < WEBRTC_ICE_RETRY_INTERVAL
            {
                return;
            }

            let Some(connection) = entry.connection.as_ref().cloned() else {
                return;
            };

            entry.last_ice_retry = Some(now);
            let pending = std::mem::take(&mut entry.pending_ice);
            (connection, pending)
        };

        let runtime = self.ensure_network_runtime();
        let mut leftovers = Vec::new();
        for candidate in pending.drain(..) {
            let init = RTCIceCandidateInit {
                candidate: candidate.candidate.clone(),
                sdp_mid: candidate.sdp_mid.clone(),
                sdp_mline_index: candidate.sdp_mline_index,
                username_fragment: None,
            };

            let result = runtime.block_on(async {
                connection
                    .add_ice_candidate(init.clone())
                    .await
                    .map_err(|err| err.to_string())
            });

            if let Err(err) = result {
                log::warn!(
                    "[webrtc] failed to add ICE candidate for {}: {err}. retaining for retry",
                    peer_id.0
                );
                leftovers.push(candidate);
            }
        }

        if let Some(entry) = self.webrtc_peers.get_mut(peer_id) {
            if leftovers.is_empty() {
                entry.last_ice_retry = None;
            } else {
                entry.last_ice_retry = Some(Instant::now());
            }
            entry.pending_ice = leftovers;
        }
    }

    fn retry_local_offer(&mut self, peer_id: &PeerId) -> Result<(), String> {
        let Some(local_peer) = self.local_signaling_peer.clone() else {
            return Err("local signaling peer unavailable".into());
        };

        let offer = self.create_local_offer(peer_id)?;
        self.signaling_send_offer(&local_peer, peer_id, offer)
            .map_err(|err| err.to_string())?;

        if let Some(entry) = self.webrtc_peers.get_mut(peer_id) {
            entry.offer_attempts = entry.offer_attempts.saturating_add(1);
            entry.last_offer_retry = Some(Instant::now());
            entry.last_event = Instant::now();
        }

        Ok(())
    }

    fn record_local_ice_candidate(&mut self, peer_id: &PeerId, candidate: &IceCandidate) {
        self.record_ice_candidate_with_origin(peer_id, candidate, IceCandidateOrigin::Local);
    }

    fn record_remote_ice_candidate(&mut self, peer_id: &PeerId, candidate: &IceCandidate) {
        self.record_ice_candidate_with_origin(peer_id, candidate, IceCandidateOrigin::Remote);
    }

    fn record_ice_candidate_with_origin(
        &mut self,
        peer_id: &PeerId,
        candidate: &IceCandidate,
        origin: IceCandidateOrigin,
    ) {
        let kind = classify_ice_candidate(&candidate.candidate);
        let entry = self.ensure_webrtc_entry(peer_id);
        match origin {
            IceCandidateOrigin::Local => {
                entry.local_candidate_types.insert(kind);
            }
            IceCandidateOrigin::Remote => {
                entry.remote_candidate_types.insert(kind);
            }
        }

        match kind {
            IceCandidateKind::ServerReflexive => entry.saw_srflx_candidate = true,
            IceCandidateKind::Relay => entry.saw_relay_candidate = true,
            _ => {}
        }

        if entry.ice_validation_started.is_none() {
            entry.ice_validation_started = Some(Instant::now());
        }
    }

    fn mark_webrtc_failed(&mut self, peer_id: PeerId, reason: &str) {
        log::warn!(
            "[webrtc] marking negotiation with {} as failed: {}",
            peer_id.0,
            reason
        );

        let now = Instant::now();
        let mut connection_to_close = None;
        if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
            connection_to_close = entry.connection.take();
            entry.state = WebRtcConnectionPhase::Failed;
            entry.last_event = now;
            entry.pending_remote_sdp = None;
            entry.pending_ice.clear();
            entry.negotiation_started = None;
            entry.connected_since = None;
            entry.offer_attempts = 0;
            entry.last_offer_retry = None;
            entry.last_ice_retry = None;
            entry.transport_attached = false;
            entry.next_reconnect_at = Some(now + WEBRTC_RECONNECT_DELAY);
            entry.ice_validation_started = None;
            entry.local_candidate_types.clear();
            entry.remote_candidate_types.clear();
            entry.saw_srflx_candidate = false;
            entry.saw_relay_candidate = false;
        }

        if let Some(connection) = connection_to_close {
            let runtime = self.ensure_network_runtime();
            if let Err(err) = runtime.block_on(async move { connection.close().await }) {
                log::warn!(
                    "[webrtc] failed to close RTCPeerConnection for {} after failure: {err}",
                    peer_id.0
                );
            }
        }

        if self.active_webrtc_peer.as_ref() == Some(&peer_id) {
            self.shutdown_active_webrtc_transport();
        }
    }

    fn tick_webrtc_negotiation(&mut self) {
        let now = Instant::now();
        let mut offer_retries: Vec<PeerId> = Vec::new();
        let mut ice_retries: Vec<PeerId> = Vec::new();
        let mut stale: Vec<(PeerId, &'static str)> = Vec::new();
        let mut reconnect: Vec<PeerId> = Vec::new();
        let mut postpone_reconnect: Vec<PeerId> = Vec::new();

        for (peer_id, entry) in &self.webrtc_peers {
            match entry.state {
                WebRtcConnectionPhase::NegotiatingOffer
                | WebRtcConnectionPhase::AwaitingRemoteAnswer => {
                    if entry.initiated_by_local
                        && entry.offer_attempts > 0
                        && entry.offer_attempts < WEBRTC_OFFER_RETRY_MAX
                        && entry
                            .last_offer_retry
                            .map(|last| now.duration_since(last) >= WEBRTC_OFFER_RETRY_INTERVAL)
                            .unwrap_or(true)
                    {
                        offer_retries.push(peer_id.clone());
                    } else if entry.initiated_by_local
                        && entry.offer_attempts >= WEBRTC_OFFER_RETRY_MAX
                        && !stale.iter().any(|(id, _)| id == peer_id)
                    {
                        stale.push((peer_id.clone(), "offer retries exhausted"));
                    }
                }
                WebRtcConnectionPhase::AwaitingIceCompletion => {
                    if !entry.pending_ice.is_empty()
                        && entry
                            .last_ice_retry
                            .map(|last| now.duration_since(last) >= WEBRTC_ICE_RETRY_INTERVAL)
                            .unwrap_or(true)
                    {
                        ice_retries.push(peer_id.clone());
                    }
                }
                _ => {}
            }

            if matches!(
                entry.state,
                WebRtcConnectionPhase::NegotiatingOffer
                    | WebRtcConnectionPhase::AwaitingRemoteAnswer
                    | WebRtcConnectionPhase::AwaitingLocalAnswer
                    | WebRtcConnectionPhase::AwaitingIceCompletion
            ) && entry
                .negotiation_started
                .map(|started| now.duration_since(started) >= WEBRTC_NEGOTIATION_STALE_AFTER)
                .unwrap_or(false)
                && !stale.iter().any(|(id, _)| id == peer_id)
            {
                stale.push((peer_id.clone(), "negotiation stale"));
            }

            if matches!(
                entry.state,
                WebRtcConnectionPhase::NegotiatingOffer
                    | WebRtcConnectionPhase::AwaitingRemoteAnswer
                    | WebRtcConnectionPhase::AwaitingLocalAnswer
                    | WebRtcConnectionPhase::AwaitingIceCompletion
            ) && entry
                .ice_validation_started
                .map(|started| now.duration_since(started) >= WEBRTC_ICE_VALIDATION_TIMEOUT)
                .unwrap_or(false)
                && !entry.saw_srflx_candidate
                && !entry.saw_relay_candidate
                && !stale.iter().any(|(id, _)| id == peer_id)
            {
                stale.push((peer_id.clone(), "no srflx/relay candidates"));
            }

            if matches!(
                entry.state,
                WebRtcConnectionPhase::Failed
                    | WebRtcConnectionPhase::Closed
                    | WebRtcConnectionPhase::Closing
            ) && let Some(deadline) = entry.next_reconnect_at
                && deadline <= now
            {
                let can_initiate = self
                    .local_signaling_peer
                    .as_ref()
                    .map(|local| local.0 < peer_id.0)
                    .unwrap_or(false);

                if can_initiate {
                    reconnect.push(peer_id.clone());
                } else {
                    postpone_reconnect.push(peer_id.clone());
                }
            }
        }

        for peer_id in offer_retries {
            match self.retry_local_offer(&peer_id) {
                Ok(()) => {
                    log::info!(
                        "[webrtc] re-sent SDP offer to {} (attempt {})",
                        peer_id.0,
                        self.webrtc_peers
                            .get(&peer_id)
                            .map(|entry| entry.offer_attempts)
                            .unwrap_or_default()
                    );
                }
                Err(err) => {
                    log::error!("[webrtc] failed to retry offer for {}: {err}", peer_id.0);
                    self.mark_webrtc_failed(peer_id, "offer retry failed");
                }
            }
        }

        for peer_id in ice_retries {
            self.flush_pending_ice(&peer_id);
        }

        for (peer_id, reason) in stale {
            self.mark_webrtc_failed(peer_id, reason);
        }

        for peer_id in postpone_reconnect {
            if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
                entry.next_reconnect_at = Some(now + WEBRTC_RECONNECT_DELAY);
            }
        }

        for peer_id in reconnect {
            if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
                entry.next_reconnect_at = Some(now + WEBRTC_RECONNECT_DELAY);
            }
            self.initiate_webrtc_connection(peer_id);
        }
    }

    fn snapshot_webrtc_metrics(&self) -> Option<WebRtcTelemetry> {
        let mut telemetry = WebRtcTelemetry {
            active_transport: None,
            fallback_available: self.command_transport_fallback.is_some(),
            peers: Vec::new(),
        };

        if let Some(transport) = self.command_transport.as_ref() {
            telemetry.active_transport = Some(format!("{:?}", transport.kind()));
        }

        if self.webrtc_peers.is_empty()
            && telemetry.active_transport.is_none()
            && !telemetry.fallback_available
        {
            return None;
        }

        let now = Instant::now();
        for (peer_id, entry) in &self.webrtc_peers {
            let since_last_event_ms = now
                .saturating_duration_since(entry.last_event)
                .as_secs_f32()
                * 1000.0;

            let negotiation_ms = entry.negotiation_started.map(|started| {
                let end = entry.connected_since.unwrap_or(now);
                end.saturating_duration_since(started).as_secs_f32() * 1000.0
            });

            let reconnect_after_ms = entry
                .next_reconnect_at
                .map(|deadline| deadline.saturating_duration_since(now).as_secs_f32() * 1000.0);

            let ice_metrics = WebRtcIceMetrics {
                local_sources: candidate_kind_set_to_strings(&entry.local_candidate_types),
                remote_sources: candidate_kind_set_to_strings(&entry.remote_candidate_types),
                srflx_seen: entry.saw_srflx_candidate,
                relay_seen: entry.saw_relay_candidate,
            };

            let (quality, link) = if self.active_webrtc_peer.as_ref() == Some(peer_id) {
                if let Some(CommandTransport::WebRtc(transport)) = self.command_transport.as_ref() {
                    if let Some(metrics) = transport.metrics_handle().latest() {
                        let quality = classify_link_quality(&metrics).to_string();
                        let link = WebRtcLinkMetrics {
                            latency_ms: metrics.command_latency_ms,
                            jitter_ms: metrics.jitter_ms,
                            bandwidth_kbps: metrics.command_bandwidth_bytes_per_sec * 8.0 / 1000.0,
                            packets_sent: metrics.command_packets_sent,
                            packets_received: metrics.command_packets_received,
                            compression_ratio: metrics.compression_ratio,
                        };
                        (quality, Some(link))
                    } else {
                        (String::new(), None)
                    }
                } else {
                    (String::new(), None)
                }
            } else {
                (String::new(), None)
            };

            telemetry.peers.push(WebRtcPeerSample {
                peer_id: peer_id.0.clone(),
                state: format!("{:?}", entry.state),
                initiated_by_local: entry.initiated_by_local,
                retries: entry.offer_attempts.saturating_sub(1),
                pending_ice: entry.pending_ice.len(),
                negotiation_ms,
                since_last_event_ms,
                quality,
                ice: ice_metrics,
                link,
                reconnect_after_ms,
            });
        }

        Some(telemetry)
    }

    fn bootstrap_signaling(&mut self) -> Result<(), SignalingError> {
        let config = SignalingBootstrapConfig::from_env()?;
        if config.disabled {
            log::info!("[engine] signaling bootstrap disabled via environment");
            return Ok(());
        }

        self.stop_signaling_server();
        self.signaling_endpoint = None;

        let endpoint = match config.endpoint.clone() {
            Some(url) => url,
            None => {
                let addr = self.start_signaling_server(config.bind_addr)?;
                Engine::signaling_url_from_addr(addr)?
            }
        };

        let peers = self.connect_signaling_client(
            endpoint.clone(),
            config.peer_id.clone(),
            config.room_id.clone(),
            config.timeout,
        )?;

        if peers.is_empty() {
            log::info!(
                "[engine] signaling connected as {} in room {}",
                config.peer_id.0,
                config.room_id.0
            );
        } else {
            let peer_list = peers
                .iter()
                .map(|peer| peer.0.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            log::info!(
                "[engine] signaling connected as {} in room {} ({} peers: {})",
                config.peer_id.0,
                config.room_id.0,
                peers.len(),
                peer_list
            );
        }

        self.local_signaling_peer = Some(config.peer_id);
        self.signaling_room = Some(config.room_id);
        self.signaling_endpoint = Some(endpoint);
        Ok(())
    }

    fn signaling_url_from_addr(addr: SocketAddr) -> Result<Url, SignalingError> {
        let host = match addr.ip() {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        let url = Url::parse(&format!("ws://{host}:{}/ws", addr.port()))?;
        Ok(url)
    }

    fn ensure_webrtc_entry(&mut self, peer_id: &PeerId) -> &mut WebRtcPeerEntry {
        self.webrtc_peers.entry(peer_id.clone()).or_default()
    }

    fn handle_webrtc_offer(&mut self, from: PeerId, sdp: SessionDescription) {
        {
            let entry = self.ensure_webrtc_entry(&from);
            entry.pending_remote_sdp = Some(sdp.clone());
            entry.state = WebRtcConnectionPhase::AwaitingLocalAnswer;
            entry.initiated_by_local = false;
            entry.last_event = Instant::now();
            entry.negotiation_started = Some(Instant::now());
            entry.connected_since = None;
            entry.offer_attempts = 0;
            entry.last_offer_retry = None;
            entry.last_ice_retry = None;
            entry.transport_attached = false;
            entry.pending_ice.clear();
            entry.ice_validation_started = Some(Instant::now());
            entry.next_reconnect_at = None;
            entry.local_candidate_types.clear();
            entry.remote_candidate_types.clear();
            entry.saw_srflx_candidate = false;
            entry.saw_relay_candidate = false;
        }

        match self.accept_remote_offer(&from, sdp) {
            Ok(()) => {
                log::info!(
                    "[webrtc] accepted offer from {} and sent local answer",
                    from.0
                );
            }
            Err(err) => {
                log::error!("[webrtc] failed to accept offer from {}: {err}", from.0);
                if let Some(entry) = self.webrtc_peers.get_mut(&from) {
                    entry.state = WebRtcConnectionPhase::Failed;
                    entry.last_event = Instant::now();
                }
            }
        }
    }

    fn handle_webrtc_answer(&mut self, from: PeerId, sdp: SessionDescription) {
        {
            let entry = self.ensure_webrtc_entry(&from);
            entry.pending_remote_sdp = Some(sdp.clone());
            entry.last_event = Instant::now();
            if entry.negotiation_started.is_none() {
                entry.negotiation_started = Some(Instant::now());
            }
            entry.connected_since = None;
            entry.last_ice_retry = None;
        }

        match self.apply_remote_answer(&from, sdp) {
            Ok(()) => {
                log::info!(
                    "[webrtc] applied answer from {}; awaiting ICE completion",
                    from.0
                );
            }
            Err(err) => {
                log::error!("[webrtc] failed to apply answer from {}: {err}", from.0);
                if let Some(entry) = self.webrtc_peers.get_mut(&from) {
                    entry.state = WebRtcConnectionPhase::Failed;
                    entry.last_event = Instant::now();
                }
            }
        }
    }

    fn handle_ice_candidate(&mut self, from: PeerId, candidate: IceCandidate) {
        {
            let entry = self.ensure_webrtc_entry(&from);
            entry.pending_ice.push(candidate.clone());
            entry.last_event = Instant::now();
            log::debug!(
                "[webrtc] queued ICE candidate from {} (queued: {})",
                from.0,
                entry.pending_ice.len()
            );
        }

        self.record_remote_ice_candidate(&from, &candidate);
        self.flush_pending_ice(&from);
    }

    fn initiate_webrtc_connection(&mut self, peer_id: PeerId) {
        let label = peer_id.0.clone();
        let Some(local_peer) = self.local_signaling_peer.clone() else {
            log::warn!(
                "[webrtc] cannot initiate negotiation with {label} without local signaling peer"
            );
            return;
        };

        if local_peer.0 >= peer_id.0 {
            log::debug!("[webrtc] deferring offer for {label} to lexicographically smaller peer");
            return;
        }

        let now = Instant::now();
        {
            let entry = self.ensure_webrtc_entry(&peer_id);
            match entry.state {
                WebRtcConnectionPhase::NegotiatingOffer
                | WebRtcConnectionPhase::AwaitingRemoteAnswer
                | WebRtcConnectionPhase::AwaitingIceCompletion
                | WebRtcConnectionPhase::Connected => {
                    log::debug!(
                        "[webrtc] negotiation with {label} already in progress (state: {:?})",
                        entry.state
                    );
                    return;
                }
                WebRtcConnectionPhase::Closing | WebRtcConnectionPhase::Closed => {
                    log::info!(
                        "[webrtc] restarting negotiation with {label} after previous shutdown"
                    );
                }
                WebRtcConnectionPhase::Failed => {
                    log::info!("[webrtc] retrying negotiation with {label} after failure");
                }
                WebRtcConnectionPhase::Idle | WebRtcConnectionPhase::AwaitingLocalAnswer => {}
            }
            entry.state = WebRtcConnectionPhase::NegotiatingOffer;
            entry.initiated_by_local = true;
            entry.last_event = now;
            entry.negotiation_started = Some(now);
            entry.connected_since = None;
            entry.offer_attempts = 0;
            entry.last_offer_retry = None;
            entry.last_ice_retry = None;
            entry.ice_validation_started = Some(now);
            entry.next_reconnect_at = None;
            entry.local_candidate_types.clear();
            entry.remote_candidate_types.clear();
            entry.saw_srflx_candidate = false;
            entry.saw_relay_candidate = false;
        }

        log::info!("[webrtc] initiating negotiation with {label}");

        match self.create_local_offer(&peer_id) {
            Ok(offer) => {
                let send_time = Instant::now();
                if let Err(err) = self.signaling_send_offer(&local_peer, &peer_id, offer) {
                    log::error!("[webrtc] failed to send offer to {label} via signaling: {err}");
                    self.mark_webrtc_failed(peer_id.clone(), "offer send failed");
                } else if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
                    entry.state = WebRtcConnectionPhase::AwaitingRemoteAnswer;
                    entry.last_event = send_time;
                    entry.offer_attempts = 1;
                    entry.last_offer_retry = Some(send_time);
                }
            }
            Err(err) => {
                log::error!("[webrtc] failed to create local offer for {label}: {err}");
                self.mark_webrtc_failed(peer_id.clone(), "offer creation failed");
            }
        }
    }

    fn cleanup_peer_connection(&mut self, peer_id: PeerId) {
        let label = peer_id.0.clone();
        match self.webrtc_peers.remove(&peer_id) {
            Some(mut entry) => {
                entry.state = WebRtcConnectionPhase::Closing;
                if let Some(conn) = entry.connection.take() {
                    let close_result = self
                        .ensure_network_runtime()
                        .block_on(async move { conn.close().await });
                    if let Err(err) = close_result {
                        log::warn!("[webrtc] failed to close RTCPeerConnection for {label}: {err}");
                    }
                }
                log::info!("[webrtc] cleaned up peer connection for {label}");

                if self.active_webrtc_peer.as_ref() == Some(&peer_id)
                    && matches!(
                        self.command_transport.as_ref(),
                        Some(CommandTransport::WebRtc(_))
                    )
                {
                    self.command_transport = None;
                }
                if self.active_webrtc_peer.as_ref() == Some(&peer_id) {
                    self.active_webrtc_peer = None;
                }
            }
            None => {
                log::debug!("[webrtc] cleanup requested for unknown peer {label}; ignoring");
            }
        }
    }

    fn shutdown_signaling_handle(&mut self, handle: SignalingHandle) {
        let errors = handle.drain_errors();
        for error in errors {
            eprintln!("[engine] signaling error before shutdown: {error}");
        }

        handle.shutdown();
    }
}

#[cfg(feature = "network-quic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebRtcConnectionPhase {
    Idle,
    NegotiatingOffer,
    AwaitingLocalAnswer,
    AwaitingRemoteAnswer,
    AwaitingIceCompletion,
    Connected,
    Closing,
    Closed,
    Failed,
}

#[cfg(feature = "network-quic")]
enum WebRtcRuntimeEvent {
    TransportEstablished {
        peer_id: PeerId,
        transport: WebRtcTransport,
    },
    ConnectionState {
        peer_id: PeerId,
        state: RTCPeerConnectionState,
    },
    LocalIceCandidate {
        peer_id: PeerId,
        candidate: IceCandidate,
    },
}

#[cfg(feature = "network-quic")]
#[derive(Debug)]
struct WebRtcPeerEntry {
    connection: Option<Arc<RTCPeerConnection>>,
    state: WebRtcConnectionPhase,
    pending_remote_sdp: Option<SessionDescription>,
    pending_ice: Vec<IceCandidate>,
    initiated_by_local: bool,
    last_event: Instant,
    has_local_data_channel: bool,
    transport_attached: bool,
    negotiation_started: Option<Instant>,
    connected_since: Option<Instant>,
    offer_attempts: u32,
    last_offer_retry: Option<Instant>,
    last_ice_retry: Option<Instant>,
    ice_validation_started: Option<Instant>,
    next_reconnect_at: Option<Instant>,
    local_candidate_types: HashSet<IceCandidateKind>,
    remote_candidate_types: HashSet<IceCandidateKind>,
    saw_srflx_candidate: bool,
    saw_relay_candidate: bool,
}

#[cfg(feature = "network-quic")]
impl Default for WebRtcPeerEntry {
    fn default() -> Self {
        Self {
            connection: None,
            state: WebRtcConnectionPhase::Idle,
            pending_remote_sdp: None,
            pending_ice: Vec::new(),
            initiated_by_local: false,
            last_event: Instant::now(),
            has_local_data_channel: false,
            transport_attached: false,
            negotiation_started: None,
            connected_since: None,
            offer_attempts: 0,
            last_offer_retry: None,
            last_ice_retry: None,
            ice_validation_started: None,
            next_reconnect_at: None,
            local_candidate_types: HashSet::new(),
            remote_candidate_types: HashSet::new(),
            saw_srflx_candidate: false,
            saw_relay_candidate: false,
        }
    }
}

#[cfg(feature = "network-quic")]
fn hook_transport_emitter(
    event_tx: &UnboundedSender<WebRtcRuntimeEvent>,
    peer_id: PeerId,
    connection: Arc<RTCPeerConnection>,
    channel: Arc<RTCDataChannel>,
) {
    let emission_guard = Arc::new(AtomicBool::new(false));
    let event_tx_on_open = event_tx.clone();
    let connection_on_open = Arc::clone(&connection);
    let channel_on_open = Arc::clone(&channel);
    let guard_on_open = Arc::clone(&emission_guard);
    let peer_on_open = peer_id.clone();

    channel.on_open(Box::new(move || {
        let event_tx = event_tx_on_open.clone();
        let peer_id = peer_on_open.clone();
        let connection = Arc::clone(&connection_on_open);
        let channel = Arc::clone(&channel_on_open);
        let guard = Arc::clone(&guard_on_open);
        Box::pin(async move {
            if guard
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }

            let transport = WebRtcTransport::from_parts(connection, channel);
            if event_tx
                .send(WebRtcRuntimeEvent::TransportEstablished { peer_id, transport })
                .is_err()
            {
                log::warn!("[webrtc] transport event dropped; engine likely shutting down");
            }
        })
    }));

    if channel.ready_state() == RTCDataChannelState::Open
        && emission_guard
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    {
        let transport = WebRtcTransport::from_parts(connection, channel);
        if event_tx
            .send(WebRtcRuntimeEvent::TransportEstablished { peer_id, transport })
            .is_err()
        {
            log::warn!("[webrtc] transport event dropped; engine likely shutting down");
        }
    }
}

#[cfg(feature = "network-quic")]
fn build_webrtc_api() -> Result<API, String> {
    let mut media_engine = MediaEngine::default();
    let registry = register_default_interceptors(Registry::new(), &mut media_engine)
        .map_err(|err| err.to_string())?;

    Ok(APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build())
}

#[cfg(feature = "network-quic")]
fn rtc_session_from(desc: &SessionDescription) -> Result<RTCSessionDescription, String> {
    match desc.sdp_type.to_ascii_lowercase().as_str() {
        "offer" => RTCSessionDescription::offer(desc.sdp.clone()).map_err(|err| err.to_string()),
        "answer" => RTCSessionDescription::answer(desc.sdp.clone()).map_err(|err| err.to_string()),
        "pranswer" => {
            RTCSessionDescription::pranswer(desc.sdp.clone()).map_err(|err| err.to_string())
        }
        "rollback" => {
            let mut rtc = RTCSessionDescription::default();
            rtc.sdp_type = RTCSdpType::Rollback;
            rtc.sdp = desc.sdp.clone();
            Ok(rtc)
        }
        other => Err(format!("unsupported SDP type '{other}'")),
    }
}

#[cfg(feature = "network-quic")]
fn session_description_from_rtc(desc: &RTCSessionDescription) -> SessionDescription {
    SessionDescription {
        sdp_type: desc.sdp_type.to_string(),
        sdp: desc.sdp.clone(),
    }
}

#[cfg(feature = "network-quic")]
#[derive(Clone, Debug)]
struct IceServerConfig {
    urls: Vec<String>,
    username: Option<String>,
    credential: Option<String>,
}

#[cfg(feature = "network-quic")]
impl IceServerConfig {
    fn from_url(url: &str) -> Self {
        Self {
            urls: vec![url.trim().to_string()],
            username: None,
            credential: None,
        }
    }

    fn parse_entry(entry: &str) -> Option<Self> {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parts: Vec<_> = trimmed.split('|').collect();
        match parts.len() {
            1 => Some(Self::from_url(parts[0])),
            3 => {
                let url = parts[0].trim();
                if url.is_empty() {
                    log::warn!("[webrtc] ignoring ICE server entry with empty url: {trimmed}");
                    return None;
                }
                let username = parts[1].trim().to_string();
                let credential = parts[2].trim().to_string();
                Some(Self {
                    urls: vec![url.to_string()],
                    username: if username.is_empty() {
                        None
                    } else {
                        Some(username)
                    },
                    credential: if credential.is_empty() {
                        None
                    } else {
                        Some(credential)
                    },
                })
            }
            _ => {
                log::warn!(
                    "[webrtc] unable to parse ICE server entry '{trimmed}'; expected 'url' or 'url|username|credential'"
                );
                None
            }
        }
    }

    fn to_rtc(&self) -> RTCIceServer {
        let mut server = RTCIceServer {
            urls: self.urls.clone(),
            ..Default::default()
        };
        if let Some(username) = &self.username {
            server.username = username.clone();
        }
        if let Some(credential) = &self.credential {
            server.credential = credential.clone();
            server.credential_type = RTCIceCredentialType::Password;
        }
        server
    }
}

#[cfg(feature = "network-quic")]
fn default_ice_servers() -> Vec<IceServerConfig> {
    vec![
        IceServerConfig::from_url("stun:stun.l.google.com:19302"),
        IceServerConfig::from_url("stun:global.stun.twilio.com:3478"),
    ]
}

#[cfg(feature = "network-quic")]
fn load_webrtc_ice_servers() -> Vec<IceServerConfig> {
    match env::var("THETA_WEBRTC_ICE_SERVERS") {
        Ok(raw) => {
            if raw.trim().eq_ignore_ascii_case("none") {
                log::warn!("[webrtc] ICE server list disabled via THETA_WEBRTC_ICE_SERVERS");
                Vec::new()
            } else {
                let parsed: Vec<IceServerConfig> = raw
                    .split(',')
                    .filter_map(IceServerConfig::parse_entry)
                    .collect();
                if parsed.is_empty() {
                    log::warn!(
                        "[webrtc] THETA_WEBRTC_ICE_SERVERS produced no valid entries; falling back to defaults"
                    );
                    default_ice_servers()
                } else {
                    log::info!(
                        "[webrtc] loaded {} ICE server(s) from environment",
                        parsed.len()
                    );
                    parsed
                }
            }
        }
        Err(_) => default_ice_servers(),
    }
}

#[cfg(feature = "network-quic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum IceCandidateKind {
    Host,
    ServerReflexive,
    Relay,
    PeerReflexive,
    Unknown,
}

#[cfg(feature = "network-quic")]
impl IceCandidateKind {
    fn label(self) -> &'static str {
        match self {
            IceCandidateKind::Host => "host",
            IceCandidateKind::ServerReflexive => "srflx",
            IceCandidateKind::Relay => "relay",
            IceCandidateKind::PeerReflexive => "prflx",
            IceCandidateKind::Unknown => "unknown",
        }
    }
}

#[cfg(feature = "network-quic")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IceCandidateOrigin {
    Local,
    Remote,
}

#[cfg(feature = "network-quic")]
fn classify_ice_candidate(candidate: &str) -> IceCandidateKind {
    let mut awaiting_type = false;
    for token in candidate.split_whitespace() {
        let lowered = token.trim().to_ascii_lowercase();
        if awaiting_type {
            return match lowered.as_str() {
                "host" => IceCandidateKind::Host,
                "srflx" | "serverreflexive" => IceCandidateKind::ServerReflexive,
                "relay" => IceCandidateKind::Relay,
                "prflx" | "peerreflexive" => IceCandidateKind::PeerReflexive,
                _ => IceCandidateKind::Unknown,
            };
        }
        awaiting_type = lowered == "typ";
    }
    IceCandidateKind::Unknown
}

#[cfg(feature = "network-quic")]
fn candidate_kind_set_to_strings(kinds: &HashSet<IceCandidateKind>) -> Vec<String> {
    let mut labels: Vec<String> = kinds.iter().map(|kind| kind.label().to_string()).collect();
    labels.sort();
    labels
}

#[cfg(feature = "network-quic")]
fn classify_link_quality(metrics: &TransportDiagnostics) -> &'static str {
    let latency = metrics.command_latency_ms;
    let jitter = metrics.jitter_ms;

    if latency <= 80.0 && jitter <= 15.0 {
        "excellent"
    } else if latency <= 140.0 && jitter <= 35.0 {
        "good"
    } else if latency <= 250.0 || jitter <= 75.0 {
        "degraded"
    } else {
        "poor"
    }
}

#[cfg(feature = "network-quic")]
struct SignalingBootstrapConfig {
    disabled: bool,
    bind_addr: SocketAddr,
    endpoint: Option<Url>,
    peer_id: PeerId,
    room_id: RoomId,
    timeout: Duration,
}

#[cfg(feature = "network-quic")]
impl SignalingBootstrapConfig {
    fn from_env() -> Result<Self, SignalingError> {
        let disabled = env::var("THETA_SIGNALING_DISABLED")
            .map(|value| is_env_truthy(&value))
            .unwrap_or(false);

        let bind_addr = match env::var("THETA_SIGNALING_BIND") {
            Ok(value) => value.parse().map_err(|err| {
                SignalingError::UnexpectedResponse(format!(
                    "invalid THETA_SIGNALING_BIND value '{value}': {err}"
                ))
            })?,
            Err(_) => "127.0.0.1:0"
                .parse()
                .expect("literal bind address should parse"),
        };

        let endpoint = match env::var("THETA_SIGNALING_URL") {
            Ok(value) => Some(Url::parse(&value)?),
            Err(_) => None,
        };

        let peer_id =
            PeerId(env::var("THETA_PEER_ID").unwrap_or_else(|_| format!("peer-{}", process::id())));
        let room_id = RoomId(env::var("THETA_ROOM_ID").unwrap_or_else(|_| "default".to_string()));

        let timeout = env::var("THETA_SIGNALING_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_secs(2));

        Ok(Self {
            disabled,
            bind_addr,
            endpoint,
            peer_id,
            room_id,
            timeout,
        })
    }
}

#[cfg(feature = "network-quic")]
fn is_env_truthy(value: &str) -> bool {
    let lowered = value.trim().to_ascii_lowercase();
    matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
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

        #[cfg(feature = "network-quic")]
        self.poll_signaling_events();

        #[cfg(feature = "network-quic")]
        self.drain_webrtc_runtime_events();

        #[cfg(feature = "network-quic")]
        let webrtc_metrics = {
            self.tick_webrtc_negotiation();
            self.snapshot_webrtc_metrics()
        };

        #[cfg(not(feature = "network-quic"))]
        let webrtc_metrics: Option<WebRtcTelemetry> = None;

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
            sample.set_webrtc_metrics(webrtc_metrics.clone());
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
                latest.set_webrtc_metrics(webrtc_metrics.clone());
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

    #[cfg(feature = "network-quic")]
    fn poll_signaling_events(&mut self) {
        let Some(local_peer) = self.local_signaling_peer.clone() else {
            return;
        };

        // Poll events with zero timeout to avoid blocking the frame loop
        let event = match self.signaling_next_event(&local_peer, Duration::from_millis(0)) {
            Ok(Some(event)) => event,
            Ok(None) => return,
            Err(err) => {
                log::warn!("[signaling] failed to poll events: {err}");
                return;
            }
        };

        self.signaling_events_polled = self.signaling_events_polled.wrapping_add(1);

        match event {
            SignalingResponse::Offer { from, sdp } => {
                if from == local_peer {
                    log::debug!("[signaling] ignoring self-authored offer");
                    return;
                }
                self.handle_webrtc_offer(from, sdp);
            }
            SignalingResponse::Answer { from, sdp } => {
                if from == local_peer {
                    log::debug!("[signaling] ignoring self-authored answer");
                    return;
                }
                self.handle_webrtc_answer(from, sdp);
            }
            SignalingResponse::IceCandidate { from, candidate } => {
                if from == local_peer {
                    log::debug!("[signaling] ignoring self-emitted ICE candidate");
                    return;
                }
                self.handle_ice_candidate(from, candidate);
            }
            SignalingResponse::PeerJoined { peer_id } => {
                if peer_id == local_peer {
                    log::debug!("[signaling] local peer join event ignored");
                    return;
                }
                self.initiate_webrtc_connection(peer_id);
            }
            SignalingResponse::PeerLeft { peer_id } => {
                if peer_id == local_peer {
                    log::debug!("[signaling] local peer left event ignored");
                    return;
                }
                self.cleanup_peer_connection(peer_id);
            }
            SignalingResponse::Error { message } => {
                log::error!("[signaling] server error: {message}");
            }
            SignalingResponse::Registered { .. } => {
                // Already handled during bootstrap; ignore duplicate registrations
            }
            SignalingResponse::HeartbeatAck => {
                // Ignore heartbeat acknowledgments
            }
        }
    }

    #[cfg(feature = "network-quic")]
    fn drain_webrtc_runtime_events(&mut self) {
        loop {
            match self.webrtc_event_rx.try_recv() {
                Ok(WebRtcRuntimeEvent::TransportEstablished { peer_id, transport }) => {
                    let mut attach_transport = false;
                    if let Some(entry) = self.webrtc_peers.get(&peer_id) {
                        if !entry.transport_attached {
                            attach_transport = true;
                        } else {
                            log::debug!(
                                "[webrtc] transport already attached for {}; ignoring duplicate",
                                peer_id.0
                            );
                        }
                    } else {
                        log::warn!(
                            "[webrtc] dropping transport for unknown peer {}; negotiated connection likely cleaned up",
                            peer_id.0
                        );
                    }

                    if attach_transport {
                        self.stash_current_transport_for_fallback();
                        self.attach_webrtc_transport(transport);
                        self.active_webrtc_peer = Some(peer_id.clone());
                        if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
                            let now = Instant::now();
                            entry.transport_attached = true;
                            entry.state = WebRtcConnectionPhase::Connected;
                            entry.last_event = now;
                            entry.connected_since = Some(now);
                        }
                        log::info!("[webrtc] command transport attached for peer {}", peer_id.0);
                    }
                }
                Ok(WebRtcRuntimeEvent::ConnectionState { peer_id, state }) => {
                    let mut should_shutdown = false;
                    if let Some(entry) = self.webrtc_peers.get_mut(&peer_id) {
                        let now = Instant::now();
                        let phase = match state {
                            RTCPeerConnectionState::Unspecified => WebRtcConnectionPhase::Idle,
                            RTCPeerConnectionState::New => WebRtcConnectionPhase::NegotiatingOffer,
                            RTCPeerConnectionState::Connecting => {
                                WebRtcConnectionPhase::AwaitingIceCompletion
                            }
                            RTCPeerConnectionState::Connected => WebRtcConnectionPhase::Connected,
                            RTCPeerConnectionState::Disconnected => WebRtcConnectionPhase::Closing,
                            RTCPeerConnectionState::Failed => WebRtcConnectionPhase::Failed,
                            RTCPeerConnectionState::Closed => WebRtcConnectionPhase::Closed,
                        };
                        entry.state = phase;
                        entry.last_event = now;

                        match state {
                            RTCPeerConnectionState::Connected => {
                                if entry.connected_since.is_none() {
                                    entry.connected_since = Some(now);
                                }
                                entry.transport_attached = true;
                                entry.next_reconnect_at = None;
                                entry.ice_validation_started = None;
                            }
                            RTCPeerConnectionState::Disconnected => {
                                entry.transport_attached = false;
                                entry.next_reconnect_at = Some(now + WEBRTC_RECONNECT_DELAY);
                            }
                            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                                entry.transport_attached = false;
                                entry.connected_since = None;
                                entry.ice_validation_started = None;
                                entry.next_reconnect_at = Some(now + WEBRTC_RECONNECT_DELAY);
                                should_shutdown = true;
                            }
                            _ => {}
                        }
                    } else {
                        log::debug!(
                            "[webrtc] received state update {:?} for unknown peer {}",
                            state,
                            peer_id.0
                        );
                    }

                    if should_shutdown {
                        if self.active_webrtc_peer.as_ref() == Some(&peer_id) {
                            log::info!(
                                "[webrtc] detaching command transport after {:?} state from {}",
                                state,
                                peer_id.0
                            );
                        }
                        self.shutdown_active_webrtc_transport();
                    }
                }
                Ok(WebRtcRuntimeEvent::LocalIceCandidate { peer_id, candidate }) => {
                    self.record_local_ice_candidate(&peer_id, &candidate);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
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

    #[cfg(feature = "network-quic")]
    use crate::network::signaling::PeerId;
    #[cfg(feature = "network-quic")]
    use crate::network::transport::WebRtcTransport;

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

    #[cfg(feature = "network-quic")]
    #[test]
    fn webrtc_offer_timeout_reactivates_fallback_transport() {
        struct EnvGuard;
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                unsafe {
                    std::env::remove_var("THETA_SIGNALING_DISABLED");
                }
            }
        }

        unsafe {
            std::env::set_var("THETA_SIGNALING_DISABLED", "1");
        }
        let _env_guard = EnvGuard;

        let mut engine = Engine::new();

        let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");
        let (fallback_transport, active_transport) = runtime.block_on(async {
            WebRtcTransport::pair()
                .await
                .expect("create transport pair")
        });

        engine.command_transport_fallback = Some(CommandTransport::from(fallback_transport));
        engine.command_transport = Some(CommandTransport::from(active_transport));

        let peer_id = PeerId("peer-b".into());
        engine.active_webrtc_peer = Some(peer_id.clone());

        let now = Instant::now();
        let entry = engine.ensure_webrtc_entry(&peer_id);
        entry.state = WebRtcConnectionPhase::AwaitingRemoteAnswer;
        entry.initiated_by_local = true;
        entry.offer_attempts = WEBRTC_OFFER_RETRY_MAX;
        entry.last_offer_retry = Some(now - WEBRTC_OFFER_RETRY_INTERVAL);
        entry.negotiation_started = Some(now - WEBRTC_NEGOTIATION_STALE_AFTER);

        engine.tick_webrtc_negotiation();

        assert!(engine.command_transport.is_some());
        assert!(engine.command_transport_fallback.is_none());
        assert!(engine.active_webrtc_peer.is_none());
        let peer_state = engine
            .webrtc_peers
            .get(&peer_id)
            .map(|entry| entry.state)
            .expect("peer entry present");
        assert_eq!(peer_state, WebRtcConnectionPhase::Failed);
    }
}
