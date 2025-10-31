# Theta Engine Architectural Specification

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
- **Synchronization:**
  - State replication via component change sets keyed by entity revision.
  - Component keys derive from canonical Rust type names hashed deterministically, keeping identifiers stable across builds and platforms.
  - Schema descriptor stream accompanies change sets so peers can reconcile hashed identifiers with human-readable component metadata.
  - Input prediction for latency-sensitive gestures.
  - Conflict resolution driven by CRDT-inspired command logs merged deterministically.
- **Session Management:** Lobby discovery, host migration, and user permissions for editing operations.

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

## Next Steps (Updated October 31, 2025)

### Sprint 1: Render & VR Integration (Priority: Critical)
- Wire `wgpu` swapchain outputs into OpenXR session swapchains for real headset presentation, targeting Quest 3 native rendering at 72 Hz baseline.
- Promote OpenXR provider from simulated passthrough to live action set polling and tracked pose updates, maintaining desktop fallback for CI/headless testing.
- Validate stereo rendering pipeline with GPU profiling hooks; establish frame time budgets (<11ms for 90 Hz target).

### Sprint 2: Networking Foundation (Priority: High)
- Stand up async runtime (Tokio) and integrate `quinn` QUIC transport with TLS 1.3 handshake.
- Author initial FlatBuffers schema for `SessionHello`, `ComponentDelta`, and `CommandLogEntry` messages; integrate `flatc` codegen into `build.rs`.
- Implement component ID hashing (SipHash-2-4) and schema manifest generation with deterministic registration ordering.
- Hook ECS change buffers into replication event pipeline with loopback validation tests.

### Sprint 3: Physics & Editor Foundations (Priority: Medium)
- Integrate Rapier3D behind `physics-rapier` feature flag with VR-specific wrapper layer (hand collision zones, haptic feedback hooks).
- Flesh out mesh editor data model: half-edge topology, vertex/edge/face operations, and triangulation utilities.
- Implement undo/redo command stack with ECS snapshot versioning; ensure commands serialize for network replication.
- Add basic in-headset UI panels rendered via overlay pipeline (tool selection, property inspector).

### Cross-Sprint Validation
- Establish Quest 3 APK build pipeline with symbol stripping and asset compression (<200 MB target).
- Add telemetry aggregation/export for scheduler profiling; visualize stage timings in desktop companion tool.
- Extend integration test suite: headless VR session simulation, network replication convergence, mesh editor command replay.
