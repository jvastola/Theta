# Theta Engine Architectural Specification

**Doc Guide:**
- For diagrams, see [Architecture Diagrams](architecture-diagrams.md).
- For protocol and schema details, see [Network Protocol Schema Plan](network_protocol_schema_plan.md).
- For roadmap, metrics, and test plans, see [INDEX](INDEX.md) and [Phase 5 Parallel Plan](phase5_parallel_plan.md).

## Purpose
Define the foundational architecture for the Theta Engine VR-first game engine and editor, detailing subsystems, responsibilities, data flow, and technical milestones required to reach a collaborative VR mesh-authoring experience.

## Guiding Principles
- **VR-native:** All runtime and editor loops assume stereo rendering and tracked input as first-class signals.
- **Data-oriented ECS:** Entities and components are stored in cache-friendly layouts, enabling parallel system execution.
- **Deterministic Networking:** Editor and runtime state must be reproducible across peers via authoritative replication.
- **Modular Pipelines:** Rendering, physics, editor tooling, and networking are independently testable crates with minimal coupling.
- **Tooling Symmetry:** The in-headset editor consumes the same ECS and rendering layers as the runtime, providing immediate what-you-see-is-what-you-play feedback.

## High-Level Subsystem Overview
- **engine:** Frame orchestration, scheduling, service registration, and lifecycle management.
- **ecs:** Entity allocator, component storage, system dispatcher, job graph, and change tracking.
- **render:** GPU device abstraction (wgpu/Vulkan), render graph, VR compositor bridge, and platform swapchain utilities.
- **vr:** OpenXR session management, tracking data ingestion, action/pose mapping, and haptics routing.
- **editor:** PolySketch-inspired mesh tools, command stack (undo/redo), selection/transform gizmos, and asset persistence.
- **network:** Transport (QUIC/WebRTC), replication protocol, conflict resolution, and collaborative editing sessions.

## Engine Runtime Loop
1. **Boot:** Initialize logging, configuration, and resource hotloaders.
2. **Subsystem Init:** Create ECS world, renderer, VR context, networking service, and editor controllers.
3. **Main Loop:**
   - Poll VR input and networking events.
   - Stage component mutations into ECS change buffers.
   - Execute system schedule (simulation, editor, rendering) via job graph.
   - Submit render graph to GPU, present stereo frame to VR runtime.
   - Persist state snapshots for undo/redo and replication.
4. **Shutdown:** Flush analytics/telemetry, dispose GPU/VR handles, and serialize session state.

## Scheduler & Frame Systems
- `engine::schedule` hosts the ECS `World` and drives registered systems each frame.
- Systems can be registered as structs or closure-based adapters, enabling game/editor layers to plug in at runtime.
- Stage-aware execution (`Startup → Simulation → Render → Editor`) batches exclusive systems per stage, while read-only jobs fan out in parallel using Rayon.
- Per-stage profiling captures sequential/parallel timings, records slow system warnings, and flags read-only policy violations so tooling can react in-editor.
- Core telemetry (e.g., frame counters, rolling stage averages, violation tallies, controller state) lives in ECS so runtime/editor layers and network replication can observe it uniformly, and the first replicated packet performs an insert handshake before emitting incremental updates.

## ECS Design
- **Entities:** Dense integer handles backed by generational indices to avoid ABA issues.
- **Components:** Stored in archetype tables (structure of arrays) with per-column change masks for efficient diffing.
- **Systems:** Declared with `SystemDescriptor` metadata (read/write sets, execution phase). Scheduler compiles into stages optimized for parallel execution.
- **Events:** Lightweight ring buffers for transient messaging (input gestures, network packets, render notifications).
- **Undo/Redo:** Command objects describe component mutations. ECS keeps versioned snapshots per entity to facilitate reversible operations.

## Rendering Architecture
- **Device Layer:** Wrap `wgpu` to support Vulkan/DirectX12/Metal backends with explicit control over swapchain timing.
- **Render Graph:** Declarative graph describing passes (geometry, lighting, post-processing, UI). Built per frame to allow editor overlays.
- **Stereo Pipeline:** Each frame renders left/right views with shared resource caches, foveated rendering compatibility, and timewarp/spacewarp hooks.
- **Mesh Authoring Integration:** Editor edits operate on GPU-friendly mesh buffers. Command stack updates CPU and GPU caches atomically.
- **Performance Goals:** Maintain 90 Hz at target headset resolution with GPU profiling hooks exposed to the editor.
- **Implementation Note:** Optional `render-wgpu` feature boots a `wgpu` backend that renders into persistent per-eye swapchain images, reusing textures via in-flight fences and emitting GPU submissions for the VR compositor.

## VR Integration
- **OpenXR Runtime:** Session creation, reference space management, action sets for controllers, and event polling. A feature-gated `vr-openxr` module now loads the runtime when available, falling back to a simulated provider otherwise.
- **Input Abstraction:** Map tracked poses, button states, and analog values into ECS components (e.g., `TrackedPose`, `ControllerState`). Simulation provider supplies wobble/trigger waveforms so systems can be exercised without hardware.
- **Haptics:** Provide command buffer for vibration patterns tied to editor actions (snap confirmation, surface contact).
- **Safety Layer:** Guardian boundary visualization and comfort settings (snap turning, vignette).

## Editor Architecture
- **Mesh Model:** Half-edge topology supporting dynamic vertex/edge/face edits, triangulation via winding order, and surface smoothing.
- **Tools:**
  - Draw/Extrude: Gesture-based creation of vertices and faces.
  - Manipulate: Grab, scale, rotate with snapping controls.
  - Duplicate/Mirror: Symmetric editing workflows.
  - History: Undo/redo with branching timelines for collaborative merges.
- **UI:** In-headset panels rendered via overlay pipeline; optional desktop companion for detailed inspection.
- **Persistence:** Save/load meshes as glTF + custom metadata for tool scripts, serialized via ECS snapshots.

## Networking Architecture
- **Transport:** QUIC (native) with WebRTC fallback. Reliability layers for ECS delta compression.
  - **Runtime Management:** Shared Tokio runtime (`Arc<TokioRuntime>`) coordinates all async networking tasks (QUIC streams, WebRTC data channels, signaling WebSocket). Engine manages runtime lifecycle via `ensure_network_runtime()` and `set_network_runtime()`, eliminating borrow conflicts by capturing the runtime before transport access.
- **Synchronization:**
  - State replication via component change sets keyed by entity revision.
  - Component keys derive from canonical Rust type names hashed deterministically, keeping identifiers stable across builds and platforms.
  - Schema descriptor stream accompanies change sets so peers can reconcile hashed identifiers with human-readable component metadata.
  - Input prediction for latency-sensitive gestures.
  - Conflict resolution driven by CRDT-inspired command logs merged deterministically.
- **Session Management:** Lobby discovery, host migration, and user permissions for editing operations.
- **Signaling Bootstrap:** With `network-quic` enabled the engine now brings up a local WebSocket signaling endpoint at startup, registers the local peer, and publishes the resulting metrics. Override the behavior with `THETA_SIGNALING_URL` (external server), `THETA_SIGNALING_BIND` (bind address), `THETA_PEER_ID`, `THETA_ROOM_ID`, `THETA_SIGNALING_TIMEOUT_MS`, or disable entirely via `THETA_SIGNALING_DISABLED=1`.
- **Voice Integration:**
  - **Codec:** Opus-based voice encoding (48 kHz mono, 20 ms frames, ~24 kbps target) with configurable bitrate for bandwidth management.
  - **Jitter Buffer:** Packet reordering buffer (16-frame default capacity) smooths network jitter and out-of-order delivery; oldest excess packets automatically dropped.
  - **VAD (Voice Activity Detection):** RMS-based energy threshold filters silent frames; only voiced packets increment telemetry counters (`voiced_frames`).
  - **Playback:** CPAL-driven audio output with automatic sample rate conversion and multi-channel mixing; gracefully degrades to silent mode when no audio device is available.
  - **Telemetry:** Per-transport voice metrics (`VoiceDiagnostics`) track packets sent/received, bytes transferred, bitrate, latency, dropped packets, and voiced frame counts; surfaced via `WebRtcTelemetry` in `FrameTelemetry` snapshots.
  - **Synthesis:** Engine synthesizes sine-wave test audio each frame for loopback validation; voice session decodes incoming packets and queues samples for playback.

## GPU Optimization Focus Areas
- Batched component uploads using persistently mapped buffers.
- Render graph pass fusion and async compute for mesh processing.
- GPU-driven selection highlighting and mesh boolean previews.
- Foveated rendering research path via OpenXR extensions.

## Testing Strategy
- **Unit Tests:** Core ECS operations, math utilities, command stack behaviors.
- **Integration Tests:** Headless frame loop, network replication (loopback), renderer smoke tests via wgpu offscreen backend.
- **Performance Harness:** Benchmarks for system scheduling, mesh editing operations, and networking serialization.
- **CI:** GitHub Actions with caching, linting (`cargo fmt`, `clippy`), unit/integration runs, and artifact publishing for nightly builds.

## Milestones
1. **MVP Runtime:** ECS with scheduler-managed systems, renderer bootstrap, VR stub, command line launch.
2. **Mesh Editing Alpha:** Half-edge data model, gesture-based creation, local undo/redo.
3. **VR Interaction Beta:** OpenXR input, stereo rendering, basic comfort settings.
4. **Multiplayer Collaboration:** Deterministic replication, session hosting, conflict resolution.
5. **Performance Pass:** GPU optimizations, profiling tools, editor UX polish.

## Resolved Design Decisions

### Physics Integration Strategy
**Decision:** Adopt Rapier as the long-term physics backend with VR-specific optimizations layered atop.

**Rationale:**
- Rapier provides proven performance on mobile ARM (Quest targets) with SIMD-optimized solvers and minimal allocations.
- Custom solver development deferred indefinitely; engineering resources better spent on unique VR interaction mechanics.
- VR-specific enhancements implemented as wrapper layers: hand collision zones with haptic feedback, predictive grab points, and comfort-mode smoothing for physics-driven cameras.
- Feature-gate Rapier behind `physics-rapier` to preserve testability and allow future backend swaps if requirements evolve.

### Hardware Target Priorities
**Decision:** Optimize primarily for standalone Quest 3 (Snapdragon XR2 Gen 2); maintain PCVR compatibility via feature flags.

**Rationale:**
- Quest 3 represents largest VR user base and harshest performance envelope (72-90 Hz, 8 GB shared memory, thermal throttling).
- Build size constraints: target <200 MB APK via aggressive symbol stripping, asset compression, and optional feature exclusion.
- PCVR mode (`target-pcvr` feature) enables higher-fidelity rendering, denser ECS simulations, and relaxed thermal budgets for tethered/Link sessions.
- Cross-platform testing matrix: Quest 3 native, Quest 3 Link, and SteamVR (Index/Vive) to validate OpenXR portability.
- Future headset support (Apple Vision Pro, Pico) added opportunistically once Quest baseline is stable.

### Collaborative Security & Authentication
**Decision:** Implement role-based permissions (viewer, editor, admin) with Ed25519-signed command entries; defer centralized auth server to post-MVP.

**Rationale:**
- Initial sessions use peer-generated keypairs exchanged during handshake; sufficient for trusted small-group collaboration.
- Role enforcement occurs at command validation layer: `CommandLogEntry` signatures verified against session roster before ECS application.
- Transport security (QUIC TLS, WebRTC DTLS) protects against network eavesdropping; signed commands prevent impersonation.
- Post-MVP: introduce optional OAuth2/OIDC integration for enterprise deployments requiring centralized identity; role mappings flow through `SessionAcknowledge` claims.
- Account for WebRTC peer-to-peer topology: clients cross-verify signatures even when host is untrusted relay, preventing malicious command injection.

## Development Status (Updated October 31, 2025)

### ✅ Completed Phases

#### Phase 1: Foundation & Schema
- **ECS Core:** 53 unit tests covering entity lifecycle, scheduler execution, profiling
- **Renderer:** Null backend + optional wgpu backend with per-eye swapchain management
- **VR:** Simulated input provider + optional OpenXR provider (desktop fallback)
- **Schema:** FlatBuffers codegen, SipHash-2-4 component hashing, manifest validation

#### Phase 2: QUIC Transport & Handshake
- **Transport:** Connection pooling, stream isolation, TLS 1.3 security
- **Handshake:** SessionHello/Acknowledge with capability negotiation, Ed25519 key exchange
- **Heartbeat:** RTT/jitter metrics, telemetry integration
- **Tests:** 8 integration tests covering validation, metrics, multi-client scenarios

#### Phase 3: ECS Replication Pipeline
- **Registry:** Type-safe component registration with zero-overhead dump functions
- **Snapshots:** Chunked encoding (16 KB default), deterministic ordering
- **Deltas:** Three-way diffing (Insert/Update/Remove), descriptor advertisement
- **Tests:** 11 unit tests + 3 integration tests (59 total across all modules)

#### Phase 4: Command Log & Conflict Resolution (✅ Complete)
- **Core:** Lamport ordering, role enforcement, conflict strategies delivered
- **Signatures:** Ed25519 signing/verification in place with pluggable traits
- **Integration:** CommandPipeline, Outbox, TransportQueue, QUIC send/receive, remote apply
- **Telemetry:** Command metrics surfaced in overlay + diagnostics; extended editor command vocabulary landed

### 🔄 Current Sprint (Nov 1-14, 2025): Phase 5 Kickoff – Production Hardening

#### Sprint Objectives
1. **Security & Integrity**
   - Implement nonce-based replay protection in `CommandPacket`
   - Add per-author token bucket rate limiting and queue depth alerting
   - Introduce payload size guards (64 KB) with telemetry reporting

2. **Transport Resilience**
   - Prototype WebRTC data-channel fallback path
   - Establish automated QUIC/WebRTC loopback convergence tests

3. **Performance & Compression**
   - Integrate Zstd compression for commands (target ≥50% reduction)
   - Stand up nightly performance benchmarks with telemetry export

4. **Documentation & Protocol**
   - Publish editor command protocol schema (Phase 4 carryover)
   - Update architecture diagrams to reflect new telemetry surfaces

### 📋 Upcoming Sprints (Nov-Dec 2025)

#### Phase 5: Production Hardening (Nov 1-28)
- Security: Command replay protection, rate limiting, payload guards
- Transport: WebRTC fallback, convergence suite, automated soak tests
- Performance: Zstd compression, telemetry export, benchmarking harness
- Docs: Editor command protocol schema, updated diagrams, operator runbooks
- Detailed parallel assignments: see `docs/phase5_parallel_plan.md`

#### Phase 6: Mesh Editor Alpha (Nov 29 - Dec 19)
- Half-edge mesh data model with boundary tracking
- Core editing operations (vertex create, edge extrude, face subdivide)
- Undo/redo command stack with collaborative branching
- In-headset UI (tool palette, property inspector)
- glTF persistence with custom metadata

#### Phase 7: OpenXR Live & Quest 3 Native (Dec 20 - Jan 16, 2026)
- Live action set polling and reference space management
- wgpu → OpenXR swapchain binding for native rendering
- Haptic feedback and comfort settings
- Quest 3 APK build pipeline (<200 MB, 72 Hz baseline)

### Current Metrics (Nov 6, 2025)
- **Tests Passing:** 87 (81 unit + 6 integration; Phase 4 + Phase 5 transport/telemetry/voice extensions validated). Enabling `network-quic` runs 13 additional transport tests (100 total with voice integration suite).
- **Test Failures:** 0
- **Phase 4 Completion:** 100%
- **Lines of Code:** ~9,200 (src + tests, including voice module)
- **Feature Coverage:** Core ECS, networking, telemetry, voice pipeline complete; mesh editor pending
