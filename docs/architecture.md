# Codex Engine Architectural Specification

## Purpose
Define the foundational architecture for the Codex VR-first game engine and editor, detailing subsystems, responsibilities, data flow, and technical milestones required to reach a collaborative VR mesh-authoring experience.

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
- Core telemetry (e.g., frame counters, stage durations, controller state) lives in ECS so runtime/editor layers and network replication can observe it uniformly.

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

## Open Questions
- Which physics library (if any) complements the VR interactions? (e.g., Rapier vs. custom)
- Target headset priority (Quest standalone vs. PCVR) informs rendering backend choices.
- Security model for collaborative editing beyond trusted peers.

## Next Steps
- Drive `wgpu` integration toward headset compositor presentation (OpenXR swapchains), building atop the new per-eye texture reuse and GPU submission plumbing.
- Promote the OpenXR provider from simulation passthrough to real action set polling and tracked pose updates, keeping the simulated fallback for desktop iteration.
- Extend scheduler profiling with aggregation/export so editor overlays can visualize stage timings and surface read-only violations.
- Draft networking protocol schema (Protobuf/FlatBuffers) for entity/component replication.
