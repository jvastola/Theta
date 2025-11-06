## ✅ Phase 4: Command Log & Conflict Resolution (Complete)

**Duration:** Weeks 8-9  
**Completion Date:** October 31, 2025  
**Outcome:** Authoritative, signed command pipeline with telemetry instrumentation

### Highlights
- Lamport-ordered command log with role-based permissions, scoped conflict strategies (last-write-wins, merge, reject), and Ed25519 signatures for every entry.
- Engine/editor integration via `CommandPipeline`, `CommandOutbox`, and `CommandTransportQueue`, providing deterministic batching and telemetry tracking without stalling the frame loop.
- QUIC replication stream now transports command packets; the engine receives, integrates, and applies remote commands (selection highlights, transform gizmo actions, tool state changes, mesh command skeletons) each frame.
- Command metrics (append rate, conflict counts, queue depth, signature verification latency) surface through the telemetry overlay and `TransportDiagnostics` snapshots.

### Test Impact
- Added 5 new tests (3 unit, 2 integration) covering transport round-trips, Lamport advancement, remote apply, and command serialization.
- **Current total:** 66 tests passing (≥ Phase 4 exit criteria of 65).

### Transition Notes
- Phase 5 inherits security and resilience follow-ups: nonce-based replay protection, rate limiting, and Zstd compression.
- Phase 4 documentation is captured in `docs/phase4_status.md`; new workstreams now drive `docs/phase5_parallel_plan.md`.


## Current System Metrics (Phase 4 Snapshot - Oct 31, 2025)
**Completion Date:** October 31, 2025 (ahead of November 7 target)

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

## Focused Priority Backlog (Promoted from Deferred)

1. **WebRTC Complexity**
  - Expand the fallback path for browser peers, including hardened signaling, TURN/STUN coverage, and convergence tests against native QUIC sessions.
  - Deliver a prototype session that proves deterministic state convergence across transports before broader rollout.

2. **Physics Integration Foundations**
  - Pull the Rapier3D integration forward to supply consistent collision volumes, haptics, and comfort-mode smoothing ahead of Phase 6 mesh tooling.
  - Establish a shared abstraction so editor and runtime systems can author tools on the same physics layer.

3. **Asset Delivery Pipeline**
  - Replace the optional CDN exploration with a concrete asset-streaming plan: resumable chunking, integrity validation, and caching for large collaborative scenes.
  - Ensure documentation and observability hooks exist so operators can monitor throughput and restart stalled transfers.

---

## Technical Debt & Future Work

### Known Limitations (To Address Post-Phase 7):
1. **Security Hardening**
   - Command replay protection (monotonic sequence IDs + nonce gossip)
   - Role-based message type filtering (enforce viewer can't send edit commands)
   - MLS group key agreement for multi-peer sessions (if WebRTC scales poorly)

2. **Mesh Editor Advanced Features**
   - CRDT-based merge strategies for concurrent mesh edits
   - GPU-accelerated boolean operations (union, subtract, intersect)
   - Procedural mesh generators (sweep, loft, revolution)

3. **Observability**
   - Audit trail persistence (encrypted command archive for compliance)
   - Analytics export (Prometheus metrics, OpenTelemetry traces)
   - Crash reporting and telemetry aggregation service

---

## Execution Streams (Date-Agnostic)

### Stream 1 — Security Hardening
- Deliver nonce-based replay protection, configurable rate limiting, and payload guard enforcement with regression and negative-path tests.
- Surface limiter and replay events through telemetry so operators can react quickly.

### Stream 2 — Transport Expansion & Compression Review
- Stand up the WebRTC data-channel prototype, demonstrate mixed QUIC/WebRTC editing sessions, and establish the compression baseline report.
- Coordinate a cross-team review to sign off on prototype stability and compression metrics before general rollout.

### Stream 3 — Hardening Closure
- Complete WebRTC fallback readiness, validate compression effectiveness targets, and land the interest-management API so replicated traffic can be scoped.
- Finalize supporting documentation (command protocol schema, operator runbook updates) alongside the technical deliverables.

---

## Success Metrics & KPIs

### Current Status (Phase 4):
- ✅ 66 tests passing (5 new tests landed during Phase 4)
- ✅ Command packets broadcast and received over QUIC replication stream
- ✅ Telemetry overlay surfaces command metrics (rate, conflicts, latency)
- ✅ Loopback convergence validated with mixed local/remote command streams
- ✅ Documentation updates published (`phase4_status.md`, Completion Summary refresh)
### Phase 4 Exit Criteria (Met):
- [x] 65+ tests passing (including transport broadcast tests)
- [x] Command packets broadcast successfully via QUIC
- [x] Telemetry overlay displays command metrics
- [x] Loopback test validates two-client command convergence
- [x] Documentation updated with command protocol schema

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

### Current Test Count (Nov 5, 2025):
```
Unit Tests:                       58 passed
Integration Tests:                 8 passed
  - command_pipeline_integration: 3 passed
  - replication_integration:      3 passed
  - telemetry_integration:        1 passed
  - transport_loopback:           1 passed

Total:                            66 passed
Failures:                          0
Ignored:                           0
```

### Test Coverage by Focus Area:
- Command Log & Pipeline: permissions, conflict resolution, Lamport ordering, fuzz, remote apply
- Replication & Schema: snapshot chunking, delta diffing, manifest hashing, descriptor advertisement
- Telemetry & Metrics: overlay rendering, command metrics snapshots, diagnostics export
- ECS & Scheduler: entity lifecycle, stage execution, profiling, violation detection
- Editor Commands & Tools: outbox lifecycle, mesh command serialization, transform/tool command flows
- Transport (QUIC): framing, command packet roundtrips, heartbeat diagnostics
- VR & Rendering: simulated input, OpenXR bridge stubs, render loop smoke tests
- Integration Suites: end-to-end command convergence, replication loopback, telemetry ingestion

---

## Revision History

| Date | Version | Author | Changes |
|------|---------|--------|---------|
| Oct 31, 2025 | 1.0 | Systems Team | Initial roadmap consolidating phases 1-4 status, November sprint plan |

