# Theta Engine Development Roadmap

**Last Updated:** October 31, 2025  
**Status:** Phase 4 In Progress

## Project Overview

Theta Engine is a Rust-native VR-first game engine and mesh authoring platform. The engine combines a data-oriented ECS architecture with GPU-accelerated rendering, OpenXR integration, and deterministic multiplayer networking to enable collaborative VR mesh editing experiences inspired by PolySketch and Google Blocks.

## Completed Milestones (Phases 1-3)

### ✅ Phase 1: Foundation & Schema (Completed)
**Duration:** Weeks 1-2  
**Status:** Production-Ready

#### Deliverables Completed:
- **ECS Core Architecture**
  - ✅ Entity-component system with generational indices
  - ✅ Stage-aware scheduler (`Startup → Simulation → Render → Editor`)
  - ✅ Parallel system execution via Rayon with read-only policy enforcement
  - ✅ Per-stage profiling with violation detection and telemetry integration
  - ✅ 53 unit tests covering ECS operations, scheduling, and profiling

- **Renderer Foundation**
  - ✅ Null backend for headless testing
  - ✅ Optional `wgpu` backend (feature: `render-wgpu`)
  - ✅ Per-eye swapchain management with in-flight fence reuse
  - ✅ VR compositor bridge hooks for stereo presentation

- **VR Integration Skeleton**
  - ✅ Simulated input provider with controller state wobble/trigger waveforms
  - ✅ Optional OpenXR provider (feature: `vr-openxr`) with desktop fallback
  - ✅ `TrackedPose` and `ControllerState` ECS components with Serde derives

- **FlatBuffers Schema System**
  - ✅ Network protocol schema (`schemas/network.fbs`)
  - ✅ Automated codegen in `build.rs` with feature-gated generation
  - ✅ Component manifest registration macros (`register_component_types!`)
  - ✅ SipHash-2-4 deterministic component ID hashing
  - ✅ CI validation for schema consistency across platforms

#### Test Coverage:
- 53 unit tests passing
- Integration tests for scheduler, renderer, and VR input simulation
- Property-based tests for deterministic hashing

---

### ✅ Phase 2: QUIC Transport & Handshake (Completed)
**Duration:** Weeks 3-4  
**Status:** Production-Ready for LAN Sessions

#### Deliverables Completed:
- **QUIC Transport Layer** (`src/network/transport.rs`)
  - ✅ Connection pooling with dedicated streams (control, replication, assets)
  - ✅ 4-byte length-prefixed framing protocol
  - ✅ Timeout-aware read/write operations
  - ✅ TLS 1.3 security with self-signed certificate generation for tests

- **Session Handshake Protocol**
  - ✅ `SessionHello` message with protocol version, schema hash, client nonce
  - ✅ `SessionAcknowledge` with session ID, role assignment, capability negotiation
  - ✅ Ed25519 public key exchange (32-byte keys)
  - ✅ Handshake validation (version/schema mismatch rejection)
  - ✅ Capability filtering (intersection of client/server feature sets)

- **Heartbeat Diagnostics**
  - ✅ Periodic heartbeat transmission (500ms default interval)
  - ✅ RTT and jitter tracking via `TransportMetricsHandle`
  - ✅ Packet sent/received counters
  - ✅ Compression ratio placeholder (awaiting Zstd integration)
  - ✅ Integration with telemetry overlay for real-time metrics

#### Test Coverage:
- 8 comprehensive integration tests covering:
  - ✅ Handshake validation (version/schema enforcement)
  - ✅ Heartbeat metrics updates within 250ms
  - ✅ Capability negotiation correctness
  - ✅ Public key exchange verification
  - ✅ Multi-client independence
  - ✅ Large payload streaming (2 MiB asset transfers)
  - ✅ Graceful shutdown on connection drop
  - ✅ Future timestamp clamping for jitter stability

#### Known Limitations:
- WebRTC fallback not yet implemented (deferred to Phase 5)
- Compression ratio metric is placeholder (awaiting Zstd)
- Single static role assignment (dynamic role changes deferred to Phase 6)

---

### ✅ Phase 3: ECS Replication Pipeline (Completed)
**Duration:** Weeks 5-7  
**Status:** Production-Ready for Snapshots & Deltas

#### Deliverables Completed:
- **Replication Registry** (`src/network/replication.rs`)
  - ✅ Type-safe component registration via `register<T: ReplicatedComponent>()`
  - ✅ Automatic deduplication via `TypeId` tracking
  - ✅ Zero-overhead dump function storage per component type
  - ✅ Reference-based API (no `Arc` cloning overhead)

- **World Snapshot Streaming**
  - ✅ Chunked encoding with configurable size limits (default: 16 KB)
  - ✅ Chunk metadata (index, total count) for ordered reassembly
  - ✅ Minimum guarantee: one component per chunk
  - ✅ Empty world handling (zero-chunk snapshots)

- **Delta Tracker**
  - ✅ Three-way diffing (Insert/Update/Remove payloads)
  - ✅ Byte-level comparison against previous frame state
  - ✅ Component descriptor advertisement with deduplication
  - ✅ Deterministic ordering (registry → archetype → component iteration)
  - ✅ Per-entity granularity with batch removal support

- **Serde Integration**
  - ✅ Serde derives on `Transform`, `Velocity`, `TrackedPose`, `ControllerState`
  - ✅ Component serialization via JSON (FlatBuffers migration pending)

#### Test Coverage:
- 11 unit tests in `network::replication`
- 3 integration tests validating:
  - ✅ Snapshot → delta handoff correctness
  - ✅ 100-entity world with multi-chunk splitting
  - ✅ 5-frame sequence (spawn, nop, update, add component, despawn)
- **Total: 59 tests passing** across all modules

#### Performance Characteristics:
- Snapshot encoding: O(n) where n = total component instances
- Delta diffing: O(n + m) for current + previous components
- Memory footprint: ~48 bytes per registered type, ~80 bytes per tracked instance

#### Known Limitations:
- JSON serialization overhead (FlatBuffers swap pending)
- No interest management filtering (all components replicated to all clients)
- No compression (Zstd integration deferred)
- Manual component registration (proc macro automation future work)

---

## 🔄 Phase 4: Command Log & Conflict Resolution (In Progress)

**Duration:** Weeks 8-9  
**Current Status:** 75% Complete  
**Target Completion:** November 7, 2025

### Completed Work:

#### ✅ Command Log Core (`src/network/command_log.rs`)
- **Lamport Clock Ordering**
  - ✅ `CommandId` with lamport counter + author ID
  - ✅ Deterministic ordering for conflict resolution
  - ✅ Clock advancement on local append and remote integration

- **Permission Enforcement**
  - ✅ Role-based command validation (Viewer/Editor/Admin)
  - ✅ `CommandRegistry` with per-command required role
  - ✅ Runtime permission checks before appending commands
  - ✅ Signature requirement enforcement

- **Conflict Strategies**
  - ✅ LastWriteWins (default for most editor commands)
  - ✅ Merge (allows concurrent edits on same scope)
  - ✅ Reject (prevents conflicting edits)
  - ✅ Per-command scope tracking (Global, Entity, Tool)

- **Signature Support**
  - ✅ Trait-based `CommandSigner` and `SignatureVerifier` abstractions
  - ✅ Ed25519 implementation (feature: `network-quic`)
  - ✅ Noop implementations for testing/development
  - ✅ Signature validation on remote command integration
  - ✅ Public key tracking per `CommandAuthor`

- **Command Storage & Replay**
  - ✅ BTreeMap-based entry storage (sorted by CommandId)
  - ✅ `entries_since()` for delta queries (late-join support)
  - ✅ `integrate_batch()` for replaying command sequences
  - ✅ `integrate_packet()` with automatic deserialization
  - ✅ Duplicate detection and rejection

#### ✅ Editor Integration (`src/editor/commands.rs`, `src/engine/commands.rs`)
- **Command Pipeline**
  - ✅ `CommandPipeline` wrapping `CommandLog` with session-aware batching
  - ✅ Selection highlight command implementation
  - ✅ Automatic batch creation on new command entries
  - ✅ `drain_packets()` API for transport consumption
  - ✅ Dynamic signer/verifier injection support

- **Command Outbox**
  - ✅ `CommandOutbox` component for batch accumulation
  - ✅ `CommandTransportQueue` for serialized packet staging
  - ✅ Batch → packet conversion with error logging
  - ✅ Telemetry tracking (total batches, entries, packets)

- **Engine Wiring**
  - ✅ Command entity registration during engine initialization
  - ✅ Frame-tick command draining from pipeline
  - ✅ Packet queueing into `CommandTransportQueue`
  - ✅ Logging of queued packets (sequence, payload size)

#### ✅ Test Coverage (Command Log)
- Unit tests:
  - ✅ Permission enforcement (viewer/editor/admin roles)
  - ✅ Conflict strategy behavior (last-write-wins, merge, reject)
  - ✅ Lamport ordering and clock advancement
  - ✅ Signature validation and rejection
  - ✅ Batch integration and replay correctness
  - ✅ Property-based fuzz harness (random interleaved commands)

- Integration tests:
  - ✅ Engine surfaces command packets after run
  - ✅ Command entity registration
  - ✅ Outbox and transport queue wiring

### 🔄 Remaining Work (Phase 4):

#### 1. Transport Broadcast (Priority: Critical)
**Target:** November 2-3, 2025

- [ ] **QUIC Stream Integration**
  - Hook `CommandTransportQueue::drain_pending()` into QUIC control stream
  - Implement `send_command_packet()` on `TransportSession`
  - Add packet sequence tracking to prevent reordering

- [ ] **Receive & Replay Pipeline**
  - Implement `receive_command_packet()` with deserialization
  - Feed packets into `CommandLog::integrate_packet()`
  - Apply integrated commands to ECS world state
  - Handle signature verification failures gracefully

- [ ] **Loopback Validation**
  - Test: host broadcasts command → client receives → world state converges
  - Test: concurrent commands from two clients merge deterministically
  - Test: signature mismatch triggers rejection (not world corruption)

#### 2. Command Metrics & Telemetry (Priority: High)
**Target:** November 4-5, 2025

- [ ] **CommandPipeline Metrics**
  - Add append rate counter (commands/sec)
  - Track conflict rejection count by strategy
  - Monitor command queue depth (backlog detection)
  - Signature verification latency histogram

- [ ] **Telemetry Overlay Integration**
  - Display command throughput in real-time
  - Show conflict rejection alerts
  - Visualize queue backlog warnings
  - Add command log diagnostics panel

- [ ] **TransportDiagnostics Extension**
  - Add `command_packets_sent` / `command_packets_received`
  - Track `command_bandwidth_bytes_per_sec`
  - Monitor `command_latency_ms` (append to remote apply)

#### 3. Additional Editor Commands (Priority: Medium)
**Target:** November 6-7, 2025

- [ ] **Transform Gizmo Commands**
  - `CMD_ENTITY_TRANSLATE` with delta position
  - `CMD_ENTITY_ROTATE` with quaternion delta
  - `CMD_ENTITY_SCALE` with scale factor

- [ ] **Mesh Editing Commands** (Skeleton)
  - `CMD_VERTEX_CREATE` with position + metadata
  - `CMD_EDGE_EXTRUDE` with source edge + direction
  - `CMD_FACE_SUBDIVIDE` with face ID + subdivision params
  - (Full mesh editor integration deferred to Phase 5)

- [ ] **Tool State Commands**
  - `CMD_TOOL_ACTIVATE` with tool ID
  - `CMD_TOOL_DEACTIVATE` cleanup

### Test Coverage Goals (Phase 4 Completion):
- Target: 65+ tests passing
- New tests:
  - Transport broadcast/receive loopback (3 tests)
  - Metrics instrumentation (2 tests)
  - Additional editor commands (3 tests)
  - Concurrent multi-client scenarios (2 tests)

---

## 📋 Upcoming Phases (November-December 2025)

### Phase 5: Production Hardening & WebRTC
**Duration:** Weeks 10-12  
**Target:** November 21, 2025

#### Planned Deliverables:
- **WebRTC Data Channel Fallback**
  - Browser peer support for web-based VR editors
  - STUN/TURN integration for NAT traversal
  - Signaling server skeleton (WebSocket-based)

- **Compression Integration**
  - Zstd dictionary training from recorded delta samples
  - Codec trait for pluggable backends
  - Metrics tracking compression effectiveness
  - Target: 50-70% reduction on typical command/delta streams

- **Interest Management Implementation**
  - Spatial cell partitioning for large worlds
  - Tool scope filtering (e.g., only replicate mesh editor state to editors)
  - Client subscription API (`subscribe_to_region`, `unsubscribe`)
  - Bandwidth optimization via selective replication

- **Loopback Convergence Suite**
  - Host + 2 clients, 120-frame simulation
  - Mixed spawn/despawn/update/command workloads
  - Packet drop injection and resync validation
  - Byte-level world state convergence assertions

- **Performance Benchmarking**
  - Criterion benchmarks for snapshot/delta/command encoding
  - Memory profiling (varying world sizes: 10-10K entities)
  - Latency percentiles (p50, p90, p99) for command apply
  - GPU profiling hooks for mesh editing previews

#### Success Criteria:
- WebRTC peers can join QUIC-hosted sessions
- Compression reduces bandwidth by ≥50% on typical workloads
- Interest management filters ≥80% of irrelevant deltas
- Loopback tests validate deterministic convergence
- Performance benchmarks establish baselines for optimization targets

---

### Phase 6: Mesh Editor Alpha
**Duration:** Weeks 13-16  
**Target:** December 19, 2025

#### Planned Deliverables:
- **Half-Edge Mesh Data Model**
  - Vertex/Edge/Face topology with boundary tracking
  - Winged-edge navigation for efficient traversal
  - Triangulation utilities with winding order preservation

- **Core Editing Operations**
  - Gesture-based vertex creation (controller raycast + trigger)
  - Edge extrusion with snap-to-grid
  - Face subdivision with Catmull-Clark smoothing option
  - Duplicate/mirror tools with symmetry axes

- **Undo/Redo Command Stack**
  - Command-based mutations (reversible operations)
  - Branching timeline support for collaborative undo
  - ECS snapshot integration for instant rewind
  - Serialization for network replication

- **In-Headset UI**
  - Tool palette overlay (floating panel)
  - Property inspector for selected entities
  - Material/color picker with haptic feedback
  - Undo/redo history visualization

- **Persistence**
  - glTF export with custom metadata extension
  - Save/load mesh scenes via ECS snapshot serialization
  - Asset versioning for collaborative editing sessions

#### Success Criteria:
- Users can create simple meshes (cube, pyramid) in VR
- Undo/redo works seamlessly across network peers
- Editor UI is readable at 1 meter distance in Quest 3
- Mesh operations maintain 90 Hz frame rate (≤11ms/frame)

---

### Phase 7: OpenXR Live Input & Quest 3 Native
**Duration:** Weeks 17-20  
**Target:** January 16, 2026

#### Planned Deliverables:
- **OpenXR Session Management**
  - Live action set polling (controllers, hands, eye tracking)
  - Reference space creation (local, stage, view)
  - Swapchain binding for Quest 3 native rendering
  - Event loop integration (session state changes, interaction profiles)

- **Stereo Rendering Pipeline**
  - wgpu swapchain → OpenXR swapchain image binding
  - Foveated rendering exploration (Qualcomm extension)
  - Timewarp/spacewarp hooks for ASW/SSW fallback
  - GPU profiling with VR frame deadlines

- **VR Interaction Refinement**
  - Haptic feedback on snap events (grid, surface contact)
  - Controller vibration for tool confirmations
  - Comfort settings (snap turning, vignette, guardian visualization)
  - Hand tracking fallback for controller-free editing

- **Quest 3 APK Build**
  - Android NDK cross-compilation pipeline
  - Symbol stripping and asset compression (<200 MB target)
  - Thermal throttling mitigation (frame rate scaling)
  - OVR metrics integration for performance monitoring

#### Success Criteria:
- Engine runs natively on Quest 3 at 72 Hz baseline
- Users can edit meshes with live controller input
- APK size ≤200 MB with all core features
- Frame times stay within thermal budget (no throttling in 10-min session)

---

## Technical Debt & Future Work

### Known Limitations (To Address Post-Phase 7):
1. **Physics Integration**
   - Rapier3D integration deferred until mesh editor stabilizes
   - VR-specific wrappers (hand collision zones, haptics) need implementation
   - Comfort-mode smoothing for physics-driven cameras pending

2. **Security Hardening**
   - Command replay protection (monotonic sequence IDs + nonce gossip)
   - Role-based message type filtering (enforce viewer can't send edit commands)
   - MLS group key agreement for multi-peer sessions (if WebRTC scales poorly)

3. **Mesh Editor Advanced Features**
   - CRDT-based merge strategies for concurrent mesh edits
   - GPU-accelerated boolean operations (union, subtract, intersect)
   - Procedural mesh generators (sweep, loft, revolution)

4. **Asset Pipeline**
   - CDN manifest pointers for large texture/mesh assets
   - Progressive download with LOD fallbacks
   - Baked lighting and occlusion culling for runtime scenes

5. **Observability**
   - Audit trail persistence (encrypted command archive for compliance)
   - Analytics export (Prometheus metrics, OpenTelemetry traces)
   - Crash reporting and telemetry aggregation service

---

## Sprint Schedule (November 2025)

### Week 9 (Nov 1-7): Phase 4 Completion
- **Nov 1-3:** QUIC command broadcast & receive implementation
- **Nov 4-5:** Command metrics & telemetry integration
- **Nov 6-7:** Additional editor commands + testing
- **Milestone:** Phase 4 complete, all command transport tests passing

### Week 10 (Nov 8-14): Phase 5 Kickoff
- **Nov 8-10:** WebRTC signaling server skeleton
- **Nov 11-12:** Zstd integration and dictionary training
- **Nov 13-14:** Interest management API design + unit tests

### Week 11 (Nov 15-21): Phase 5 Core
- **Nov 15-17:** Spatial cell partitioning implementation
- **Nov 18-19:** Loopback convergence test suite
- **Nov 20-21:** Performance benchmarking and baseline establishment

### Week 12 (Nov 22-28): Phase 5 Wrap-Up (Thanksgiving week)
- **Nov 22-24:** WebRTC peer integration testing
- **Nov 25-26:** Compression effectiveness validation
- **Nov 27-28:** Phase 5 documentation and review

---

## Success Metrics & KPIs

### Current Status (Phase 4):
- ✅ **59 tests passing** (53 unit + 6 integration)
- ✅ **0 test failures** across all modules
- ✅ **100% feature parity** on command log core vs spec
- 🔄 **75% completion** on Phase 4 overall

### Phase 4 Exit Criteria:
- [ ] 65+ tests passing (including transport broadcast tests)
- [ ] Command packets broadcast successfully via QUIC
- [ ] Telemetry overlay displays command metrics
- [ ] Loopback test validates two-client command convergence
- [ ] Documentation updated with command protocol schema

### Phase 5 Exit Criteria:
- [ ] WebRTC peers join QUIC sessions successfully
- [ ] Compression reduces bandwidth ≥50%
- [ ] Interest management filters ≥80% of irrelevant deltas
- [ ] 120-frame loopback test converges byte-for-byte
- [ ] Benchmarks establish performance baselines

### Phase 6 Exit Criteria:
- [ ] Users create simple meshes (5+ primitives) in VR
- [ ] Undo/redo maintains 90 Hz across network
- [ ] Editor UI passes readability tests (1m distance)
- [ ] Mesh operations stay ≤11ms/frame

### Phase 7 Exit Criteria:
- [ ] Quest 3 native build runs at 72 Hz
- [ ] APK size ≤200 MB
- [ ] No thermal throttling in 10-min sessions
- [ ] Live controller input drives mesh editing smoothly

---

## Risk Register

### High Risks:
1. **Quest 3 Performance Bottleneck**
   - *Likelihood:* Medium | *Impact:* High
   - *Mitigation:* Establish GPU profiling early; defer expensive features if needed
   - *Fallback:* PCVR-first deployment if mobile targets miss perf goals

2. **WebRTC Signaling Complexity**
   - *Likelihood:* Medium | *Impact:* Medium
   - *Mitigation:* Use battle-tested signaling libraries; defer custom STUN/TURN
   - *Fallback:* QUIC-only for initial release; WebRTC as post-MVP feature

### Medium Risks:
3. **Command Merge Conflicts**
   - *Likelihood:* Medium | *Impact:* Medium
   - *Mitigation:* Conservative conflict strategies; user-facing conflict resolution UI
   - *Fallback:* Last-write-wins for MVP; defer complex CRDT merges

4. **Compression Ratio Lower Than Expected**
   - *Likelihood:* Low | *Impact:* Medium
   - *Mitigation:* Benchmark early; tune dictionary training parameters
   - *Fallback:* Interest management reduces payload sizes even without compression

### Low Risks:
5. **Schema Evolution Breaking Compatibility**
   - *Likelihood:* Low | *Impact:* High
   - *Mitigation:* FlatBuffers schema versioning; CI validation tests
   - *Fallback:* Manual migration scripts for breaking changes (pre-1.0 only)

---

## Appendix: Test Summary

### Current Test Count (Oct 31, 2025):
```
Unit Tests (src/lib.rs):           53 passed
Integration Tests:                  6 passed
- command_pipeline_integration:     2 passed
- replication_integration:          3 passed
- telemetry_integration:            1 passed

Total:                             59 passed
Failures:                           0
Ignored:                            0
```

### Test Coverage by Module:
- `ecs`: 4 tests (entity lifecycle, component storage)
- `engine::schedule`: 6 tests (system execution, profiling, violations)
- `engine::commands`: 1 test (pipeline packet emission)
- `editor::commands`: 3 tests (outbox, transport queue)
- `editor::telemetry`: 5 tests (surface, replicator, overlay, sequences)
- `network::command_log`: 6 tests (permissions, conflicts, replay, fuzz)
- `network::replication`: 11 tests (snapshot, delta, chunking, registry)
- `network::schema`: 3 tests (hashing, manifest, registry)
- `network::transport`: 0 tests in lib (8 integration tests in separate file)
- `network` (misc): 5 tests (session, changeset, flatbuffers)
- `render`: 3 tests (null backend, frame ordering)
- `vr`: 3 tests (simulated input, null bridge)

---

## Revision History

| Date | Version | Author | Changes |
|------|---------|--------|---------|
| Oct 31, 2025 | 1.0 | Systems Team | Initial roadmap consolidating phases 1-4 status, November sprint plan |

