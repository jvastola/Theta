# Phase 4 Command Log & Conflict Resolution Plan

**Date:** October 31, 2025  
**Owner:** Networking & Systems Team

## Context Recap (Phase 3)
- Phase 3 delivered deterministic ECS snapshots and deltas with registry-driven serialization.
- Network session plumbing emits `ChangeSet` bundles but currently carries only component diffs.
- Command streams remain ad-hoc editor events; no ordering, deduplication, or trust guarantees exist yet.

## Phase 4 Goal
Introduce an authoritative, signed command log that merges concurrent editor actions deterministically while enforcing role-based permissions.

## Deliverables
1. **Command Log Core**
   - Lamport-clocked command IDs with deterministic ordering.
   - Conflict strategies (last-write-wins, merge, reject) applied per command scope.
   - Deduplicated log storage with replay helpers for late-joining peers.

2. **Signing & Verification**
   - Ed25519-backed signatures on every log entry.
   - Flexible verifier trait allowing alternative implementations for tests.
   - Key registry tying session participants to public keys and roles.

3. **Permission Enforcement**
   - Command registry describing required role and default conflict policy.
   - Runtime checks ensuring only authorized peers append specific commands.

4. **Engine & Editor Integration**
   - Bridge between editor tools and the new command log API.
   - Conversion of existing selection/tool commands into serialized payloads.
   - Network session wiring to broadcast signed entries alongside component diffs.

5. **Validation & Reliability**
   - Unit tests covering lamport ordering, conflict resolution, and signature failures.
   - Fuzz harness exercising random command interleavings.
   - Documentation updates describing log schema and integration steps.

## Work Breakdown Structure
1. **Foundation (Week 8)**
   - Implement `network::command_log` module with lamport IDs and conflict handling.
   - Provide trait-based signer/verifier abstractions plus Ed25519 implementation (behind `network-quic`).
   - Define `CommandRegistry` metadata and ensure role validation paths.

2. **Integration (Week 8-9)**
   - Convert editor selection/tool events into structured command payloads.
   - Hook command emission/execution into engine scheduler and telemetry replicator.
   - Extend network session serialization to deliver command log entries.

3. **Reliability (Week 9)**
   - Add fuzz tests simulating concurrent edits and verifying deterministic convergence.
   - Build replay helpers so late joiners can request missing command ranges.
   - Surface command metrics (append rate, conflict counts) through telemetry.

## Risks & Mitigations
- **Signature Overhead:** Ed25519 signing/verifying may add latency; mitigate with pre-hashed payloads and batching.
- **Command Explosion:** Naïve merge strategy could replay redundant commands; use conflict policies and scope-specific pruning.
- **Role Drift:** Mismatched role assignments could reject legitimate commands; maintain authoritative role map from session handshake.

## Definition of Done
- Command log API merged with full unit test coverage.
- Editor commands routed through the log with deterministic ordering across peers.
- Signatures verified before applying remote commands; unauthorized entries rejected.
- Documentation (`network_protocol_schema_plan.md`, `phase4_plan.md`) updated with status and schema references.
- Fuzz harness executes in CI via `cargo test --features network-quic --test command_log_fuzz` (placeholder).

## Status (Oct 31, 2025)
- Foundation: ✅ Command log core with Lamport ordering, role enforcement, and Ed25519 hooks.
- Integration: 🔄 Engine command pipeline emits selection highlight commands, routes batches into the new `CommandOutbox` ECS component, and is covered by integration tests.
- Reliability: ⬜ Fuzz harness, replay helpers, and telemetry metrics remain pending.

## Follow-Up (Phase 4.1+)
- Integrate command log snapshots with ECS snapshots for rapid rewinds.
- Add CRDT-based merge strategies tailored for PolySketch mesh operations.
- Implement audit trail persistence for compliance (encrypted command archive).
