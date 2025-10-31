# Theta Network Protocol Schema Plan

## Objectives
- Provide a deterministic, low-latency replication layer for ECS state, editor commands, and VR input streams.
- Support heterogeneous transports (native QUIC, WebRTC data channels) without leaking transport-specific details into gameplay layers.
- Maintain forward compatibility through schema versioning, negotiated capabilities, and optional field semantics.
- Enforce collaborative security via signed change sets, role-based permissions, and encrypted transport tunnels when available.

## Guiding Constraints
- Schema must be binary and compact; target sub-millisecond encode/decode for common packets under 4 KB.
- Component data derives from canonical Rust type hashes to keep identifiers stable across builds.
- Deterministic ordering for replicated commands and component diffs so peers converge without global locks.
- Support partial replication and interest management to avoid broadcasting entire world state each frame.

## Serialization Strategy
- Primary format: FlatBuffers (little-endian) for zero-copy reads and optional fields.
- Secondary fallback: CBOR for tooling/debug builds when FlatBuffers codegen is unavailable.
- Schema compiler generates Rust, C++, and TypeScript bindings to cover runtime, headless servers, and WebRTC peers.
- Include schema hash in every packet header to reject mismatched builds before applying data.

## Protocol Layers
- **Transport Envelope:** Minimal framing (message type, length, compression flag, sequence ID, CRC32). Encryption handled at transport layer (QUIC TLS 1.3, DTLS for WebRTC).
- **Session Control Layer:** Auth handshake, capability negotiation, latency probes, heartbeat, disconnect codes.
- **Replication Layer:** ECS snapshots, delta streams, command logs, telemetry metrics.
- **Collaboration Layer:** Editor tool events, chat, cursor/selection highlights, asset streaming.

## Message Catalog
1. **SessionHello / SessionAcknowledge**
   - Fields: protocol_version, schema_hash, client_nonce, requested_capabilities, auth_token (optional), client_public_key (Ed25519).
   - Response includes server_nonce, session_id, assigned_role, capability_mask, server_public_key (Ed25519).
2. **Heartbeat**
   - Ping/pong with monotonic timestamps, reported RTT, and jitter metrics for diagnostics.
3. **WorldSnapshot**
   - Sent on join or major topology change; contains entity roster, component archetype table, initial component payloads.
   - Chunked with streaming flag and chunk sequence for large worlds.
4. **ComponentDelta**
   - Array of (entity_id, component_id_hash, revision, diff_payload).
   - Supports compression modes: none, Zstd dictionary, delta-of-delta.
5. **CommandLogEntry**
   - Deterministic CRDT-style command payloads with Lamport clocks and author signature.
   - Collision resolution strategy metadata (last-writer-wins, merge, reject) per command type.
6. **InputPredictiveState**
   - VR controller poses, gesture hints, and prediction horizon metadata for latency smoothing.
7. **EditorEvent**
   - Tool activations, selection highlights, gizmo transforms, UI panel updates.
8. **AssetTransfer**
   - Mesh/texture binary chunks with SHA-256 checksums and resume tokens.
9. **TelemetryReport**
   - Frame timings, system diagnostics, and slow-frame alerts to surface in-editor dashboards.
10. **SessionControl**
    - Role changes, kick/ban notifications, migration intent, and rendezvous instructions for host handoff.

## Handshake & Capability Negotiation
- Clients send `SessionHello` with supported transports, compression options, security modes.
- Server responds with `SessionAcknowledge`, assigns session ID, and diff of negotiated features.
- Both sides derive shared keys if higher-level signing/encryption is enabled.
- Schema version mismatch triggers downgrade attempt or disconnect with remediation hint.

## Replication Flow
1. **Join Phase:** Client receives `WorldSnapshot` chunks, validates component hashes, seeds ECS state.
2. **Steady State:** Server streams `ComponentDelta` and `CommandLogEntry` messages at frame cadence.
3. **Prediction:** Clients send `InputPredictiveState` messages; server responds with authoritative corrections embedded in subsequent deltas.
4. **Conflict Handling:** Command log merges rely on CRDT metadata. Divergent states trigger targeted `WorldSnapshot` for affected entities.
5. **Interest Management:** Clients publish interest sets (spatial cells, tool scopes). Server filters deltas accordingly.

## Component Schema Definition
- Every component registered for replication defines:
  - Stable identifier: `fn stable_id() -> ComponentIdHash` (64-bit).
  - FlatBuffer table describing fields, default values, and diff strategy.
  - Diff encoder: field-level change masks, quantization strategies for floats.
- Tooling generates schema manifest (`schema_manifest.json`) mapping hashes to human-readable metadata for debugging.

## Security Considerations
- Signed command entries using Ed25519; public keys exchanged during handshake.
- Role-based permissions define allowed message types per client role (viewer, editor, admin).
- Optional end-to-end encryption for command logs when transport security is insufficient.
- Replay protection via monotonically increasing sequence IDs and nonce gossip.

## Reliability & Ordering
- QUIC streams: dedicate stream 0 for control, stream 1 for replication deltas, stream 2+ for assets.
- WebRTC channels: configure ordered/reliable for control, unordered/partial reliability for high-rate input.
- Sequence numbers per message category; clients request resends via `NackRequest` when gaps detected.

## Versioning Strategy
- Protocol version follows semver. Breaking schema updates increment major version and require handshake renegotiation.
- Component schema manifest includes `since_version`/`deprecated_in` to phase out fields gracefully.
- Introduce migration scripts to convert saved sessions when schema evolves.

## Observability & Diagnostics
- Embed trace IDs in messages for cross-service correlation.
- Optional full packet mirror logging to ring buffers with rate limiting.
- Metrics exported via telemetry layer (packet rate, delta sizes, compression ratios).

## Implementation Roadmap
1. **Phase 1 - Schema Foundation (Week 1-2)**
   - Author FlatBuffer schema files for core message families (`SessionHello`, `ComponentDelta`, `CommandLogEntry`, `AssetTransfer`).
   - Integrate `flatc` codegen into `build.rs` with Rust/TypeScript/C++ output targets.
   - Implement schema manifest generator with SipHash-2-4 component ID hashing and declarative registration macros.
   - Add CI schema compatibility checks across x86_64/aarch64/wasm32 targets.

   **Status (Oct 31, 2025):** `schemas/network.fbs` defined, automated `flatc` codegen wired via `build.rs`, runtime manifest registry with SipHash-2-4 hashing and `register_component_type!` macro implemented. CI compatibility matrix pending.

2. **Phase 2 - Transport & Handshake (Week 3-4)**
   - Implement QUIC transport layer using `quinn` with TLS 1.3 and dedicated streams (control, replication, assets).
   - Build `SessionHello`/`SessionAcknowledge` handshake with Ed25519 key exchange and capability negotiation.
   - Add heartbeat mechanism with latency probes and jitter buffer metrics.
   - Integrate transport diagnostics into telemetry layer (packet rate, compression ratios).

   **Status (Oct 31, 2025):** QUIC transport prototype available under `network-quic` feature with dedicated control/replication/asset streams. Handshake exchanges Ed25519 public keys, validates protocol/schema hash, and negotiates capabilities. Heartbeat tasks update RTT/jitter metrics, now surfaced through telemetry overlay.

3. **Phase 3 - ECS Replication Pipeline (Week 5-7)**
   - Implement `WorldSnapshot` chunked encoding/decoding with streaming support.
   - Build component delta encoder with field-level change masks and Zstd dictionary compression.
   - Add interest management system with spatial cell and tool scope filtering.
   - Wire replication events into ECS change buffers and validate deterministic convergence via loopback tests.

   **Status (Oct 31, 2025):** Replication scaffolding landed in `network::replication` with JSON-backed snapshot chunking, delta tracking, and registry-driven component serialization. Phase plan captured in `docs/phase3_plan.md`; FlatBuffers encoding, interest filtering, and engine wiring scheduled next.

4. **Phase 4 - Command Log & Conflict Resolution (Week 8-9)**
   - Layer CRDT-style command log with Lamport clocks and signature validation.
   - Implement conflict resolution strategies (last-writer-wins, merge, reject) per editor tool command type.
   - Add role-based permission checks at command validation layer.
   - Stand up protocol fuzz tests targeting command merge logic.

5. **Phase 5 - Editor Collaboration & Assets (Week 10-11)**
   - Extend schema with `EditorEvent` messages for tool activations, selection highlights, and gizmo transforms.
   - Implement `AssetTransfer` chunked streaming with SHA-256 checksums and resume tokens.
   - Integrate telemetry reporting (`TelemetryReport` messages) into in-editor dashboards.
   - Build interoperability harness testing native QUIC vs WebRTC data channel peers.

6. **Phase 6 - Production Hardening (Week 12+)**
   - Add WebRTC fallback transport with DTLS and unordered/partial reliability channels.
   - Implement session control messages (`SessionControl`) for role changes, kick/ban, host migration.
   - Document schema evolution workflow and establish automated migration scripts.
   - Run soak tests with 8-peer sessions at 90 Hz replication cadence on Quest 3 hardware.

## Resolved Design Decisions

### Asset Streaming Protocol
**Decision:** Use dedicated `AssetTransfer` messages within the same FlatBuffers schema rather than external CDN protocols.

**Rationale:**
- VR collaborative sessions need low-latency asset delivery during live editing; CDN round-trips introduce unacceptable delays.
- Reusing the QUIC transport enables zero-RTT asset resumption and leverages existing authentication/encryption.
- FlatBuffers schema unification keeps tooling simpler (single codegen pipeline, unified diagnostics).
- Optional future enhancement: add CDN manifest pointers for pre-baked assets in production builds while retaining inline streaming for editor sessions.

### Group Session Encryption
**Decision:** Rely on transport-layer security (QUIC TLS 1.3, DTLS for WebRTC) for initial implementation; defer MLS adoption pending host migration telemetry.

**Rationale:**
- TLS 1.3 provides forward secrecy and low overhead suitable for 90 Hz VR targets.
- MLS adds complexity (group key agreement, roster management) best justified by frequent host handoffs; defer until telemetry proves migration is common.
- Current role-based permissions and signed command entries provide adequate security posture for small-group collaboration (2-8 peers).
- MLS integration path remains open: handshake layer already supports capability negotiation for future security upgrades.

### Schema Hash Collision Mitigation
**Decision:** Use 64-bit SipHash-2-4 with deterministic component registration order enforced by declarative macros.

**Rationale:**
- 64-bit space makes birthday-bound collisions negligible for realistic component counts (<10,000 types).
- SipHash-2-4 is cryptographically strong against malicious inputs while maintaining sub-microsecond hashing for common component identifiers.
- Enforce stable ordering via `inventory` crate or similar registration pattern so builds produce identical hashes for matching component sets.
- Schema manifest JSON includes full type paths as collision fallback; handshake validates manifest checksums to detect ordering divergence.
- CI integration runs schema manifest diffing across target platforms (x86_64, aarch64, wasm32) to catch layout inconsistencies pre-release.
