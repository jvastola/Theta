# Phase 3 Replication Pipeline Plan

**Date:** October 31, 2025  
**Owner:** Networking & Systems Team  
**Status:** ✅ Complete (Phase 4 in Progress)

## Context Recap (Phases 1 & 2)

- **Phase 1 (Schema Foundation)** delivered the FlatBuffers catalog (`schemas/network.fbs`), automated code generation in `build.rs`, and the manifest registration macros (`register_component_types!`). Component identifiers are now deterministic via SipHash-2-4, keeping cross-build compatibility.
- **Phase 2 (Transport & Handshake)** completed QUIC transport (`network::transport`), the session handshake with capability negotiation, and integrated heartbeat diagnostics surfaced through the telemetry layer. Comprehensive integration tests cover happy paths and rejection scenarios.
- Both phases now ship with robust unit and integration coverage, ensuring that schema evolution and transport behavior are catchable via CI.

## Phase 3 Goal

Implement the ECS replication pipeline that streams initial world snapshots and incremental component deltas, establishing deterministic convergence for collaborative sessions.

## Deliverables

1. **Snapshot Streaming**
   - Chunked `WorldSnapshot` encoding with configurable payload size limits.
   - Registry-driven component serialization to keep transport independent from engine internals.
   - Tests covering chunk boundaries, empty worlds, and heterogeneous component sets.

2. **Delta Encoding**
   - `DeltaTracker` that tracks inserts/updates/removals against the previous frame.
   - Change sets emitted as `ComponentDiff` payloads with descriptor advertisements for new component types.
   - Deterministic ordering guarantees for stable replay.

3. **Interest Management Skeleton**
   - Abstractions for spatial cells/tool scopes (no filtering logic yet) so clients can declare interests.
   - API hooks for future Stage 3.2 work (cell assignment, tool scope registry).

4. **Engine Integration (First Pass)**
   - Frame hook in `engine::Engine` that produces replication deltas each tick when networking is enabled.
   - Publish pipeline bridging `DeltaTracker` output with `TelemetryReplicator`/`NetworkSession`.
   - Feature-gated to avoid impacting headless tests when networking is disabled.

5. **Diagnostics & Tooling**
   - Metrics for snapshot size, delta size, and compression effectiveness (placeholder until Zstd lands in Phase 3.2).
   - Editor overlay card surfacing replication throughput.

## Work Breakdown Structure

1. **Foundation (completed today)**
   - `network::replication` module with `ReplicationRegistry`, `WorldSnapshotBuilder`, and `DeltaTracker`.
   - Serde derives added to core replicated components (`Transform`, `Velocity`, VR tracked state).
   - Unit tests for chunking and delta flows.

2. **Serialization Backing (Week 5)**
   - Swap JSON placeholder with FlatBuffers tables (`WorldSnapshotChunk`, `ComponentDelta`).
   - Add schema entries and regenerate bindings.
   - Verify big-endian/little-endian neutrality in tests.

3. **Engine Wiring (Week 5)**
   - Instantiate a global `ReplicationRegistry` during engine boot.
   - Register core ECS components (transforms, velocity, tracked poses, telemetry surfaces as needed).
   - Attach `DeltaTracker` to scheduler tick; feed diffs into `NetworkSession::craft_change_set`.

4. **Interest Management API (Week 6)**
   - Define `InterestRegionId`, `ToolScopeId`, and client subscription messages.
   - Provide server-side registry with stub filters (currently pass-through).
   - Unit tests exercising registration/unregistration flows.

5. **Compression & Metrics (Week 6)**
   - Add byte counters and rolling averages for snapshot/delta payloads.
   - Lay out trait for compression codecs (no-op + hook for future Zstd integration).
   - Surface metrics through `TransportDiagnostics`.

6. **Integration Testing (Week 7)**
   - Loopback harness: host + two clients verifying deterministic convergence across 120 simulated frames.
   - Failure injection: dropped packets, reordered deltas, snapshot restart on divergence.
   - Ensure registry handles component removals during host migration simulation.

## Risks & Mitigations

- **Serialization Swap (JSON → FlatBuffers):** keep the trait boundary (`ReplicatedComponent`) abstract so encoder choice is localized; start with JSON for test clarity, swap once schema tables exist.
- **Component Coverage:** baseline registry only handles explicitly registered types; provide compile-time macro helpers in later sprint to reduce omissions.
- **Delta Growth:** without compression, large worlds may exceed MTU; chunking and planned Zstd integration mitigate this, with metrics to highlight spillover.

## Definition of Done

✅ **All criteria met:**
- Engine produces snapshots and deltas suitable for network transmission behind `network-quic` feature.
- Tests cover chunking, delta sequencing, removal handling, and basic interest subscription APIs.
- Documentation updated (`docs/network_protocol_schema_plan.md` and this plan) with status and follow-up tasks.
- Telemetry surfaces replication bandwidth statistics.
- Registry refactored to reference-based API (no Arc cloning).
- 59 tests passing across all modules (53 unit + 6 integration).

## Follow-Up (Phase 3.2+)

- Zstd dictionary training and codec integration.
- Spatial grid implementation for interest management.
- Mixed transport validation (QUIC ↔ WebRTC) once Phase 5 scaffolding exists.
- Integration of command log replication alongside component deltas (Phase 4 dependency).
