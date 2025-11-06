# Theta Engine: Universal Development Plan

**Date-Agnostic Execution Framework**  
**Scope:** VR-first collaborative game engine & mesh editor with multiplayer voice

---

## Executive Summary

Theta Engine is a Rust-native VR collaboration platform combining real-time mesh authoring, deterministic command replication, and integrated voice communication. This plan consolidates all architectural decisions, test coverage requirements, and delivery milestones into a universal execution roadmap independent of specific dates.

**Current State:** Phase 4 complete (Command Log & Conflict Resolution), Phase 5 in-flight (Production Hardening & Transport Resilience)  
**Test Coverage:** 74 tests passing (68 unit + 6 integration; 86 with `network-quic` feature)  
**Lines of Code:** ~8,500 source + ~2,700 test = ~11,200 total

---

## Architecture Foundation

### Core Subsystems

#### 1. Entity-Component-System (ECS)
- **Location:** `src/ecs/mod.rs`
- **Status:** ✅ Production-ready
- **Features:**
  - Generational entity indices (ABA-safe)
  - HashMap-based component storage per type
  - Registration macro system with compile-time safety
  - Parallel query execution via Rayon
  - Change tracking for replication delta computation

#### 2. Scheduler & Frame Loop
- **Location:** `src/engine/schedule.rs`
- **Status:** ✅ Production-ready
- **Stages:** Startup → Simulation → Render → Editor
- **Features:**
  - Per-stage profiling (sequential/parallel time tracking)
  - Read-only policy enforcement with violation detection
  - Deterministic execution order
  - Telemetry integration for frame diagnostics

#### 3. Rendering Pipeline
- **Location:** `src/render/mod.rs`
- **Status:** ✅ Foundation complete
- **Backends:**
  - Null backend (headless CI testing)
  - wgpu backend (`render-wgpu` feature)
- **Features:**
  - Per-eye swapchain management
  - In-flight fence coordination
  - VR compositor bridge hooks
  - GPU frame pacing

#### 4. VR Integration
- **Location:** `src/vr/mod.rs`, `src/vr/openxr.rs`
- **Status:** ✅ Foundation complete
- **Providers:**
  - Simulated input (default, controller wobble + trigger waves)
  - OpenXR provider (`vr-openxr` feature)
- **Features:**
  - Desktop fallback when runtime unavailable
  - TrackedPose and ControllerState components
  - Haptic feedback hooks (ready for Phase 6+)

#### 5. Network Transport
- **Location:** `src/network/transport.rs`
- **Status:** ✅ QUIC complete, WebRTC prototype ready
- **Transports:**
  - **QUIC:** Primary (TLS 1.3, stream isolation, heartbeat diagnostics)
  - **WebRTC:** Fallback (in-memory channel prototype, real WebRTC pending)
- **Features:**
  - Unified `CommandTransport` enum abstraction
  - Metrics tracking (RTT, jitter, bandwidth, packet counts)
  - Transport kind telemetry (`TransportKind::Quic` | `WebRtc`)
  - Ed25519 key exchange during handshake
  - Capability negotiation (client/server feature intersection)

#### 6. Command Log & Conflict Resolution
- **Location:** `src/network/command_log.rs`
- **Status:** ✅ Production-ready
- **Features:**
  - Lamport clock ordering for deterministic conflict resolution
  - Role-based permissions (Viewer/Editor/Admin)
  - Conflict strategies (LastWriteWins, Merge, Reject)
  - Ed25519 signature support with trait-based verification
  - Nonce-based replay protection with high-water mark tracking
  - Token-bucket rate limiting (configurable thresholds)
  - 64 KiB payload guards with telemetry
  - Late-join support via `entries_since` delta queries

#### 7. Replication Pipeline
- **Location:** `src/network/replication.rs`
- **Status:** ✅ Production-ready
- **Features:**
  - Chunked world snapshots (configurable size limits, default 16 KB)
  - Delta tracker (Insert/Update/Remove diffs with byte-level comparison)
  - Component descriptor advertisement with deduplication
  - Registry-driven serialization (JSON placeholder, FlatBuffers ready)
  - Deterministic ordering (registry → archetype → component iteration)

#### 8. Telemetry & Diagnostics
- **Location:** `src/editor/telemetry.rs`
- **Status:** ✅ Production-ready
- **Features:**
  - Frame-by-frame profiling with rolling averages
  - Transport diagnostics (kind, RTT, jitter, packet counts, bandwidth)
  - Command metrics (append rate, conflicts, queue depth, signature latency)
  - Security metrics (replay rejections, rate-limit drops, payload guards)
  - Real-time overlay rendering
  - Replication to remote operators via `TelemetryReplicator`

---

## Multiplayer Voice Communication (New Scope)

### Design Philosophy
Integrate real-time voice communication as a first-class transport concern, leveraging existing WebRTC infrastructure and command/telemetry patterns. Voice streams should coexist with command replication and mesh editing without blocking the render loop.

### Architecture

#### Voice Transport Layer
- **Location:** `src/network/voice.rs` (new module)
- **Integration:** Extend `CommandTransport` with voice-specific data channels
- **Encoding:** Opus codec (low-latency, VoIP-optimized, 20ms frames)
- **Packet Priority:** Voice packets bypass command queue; dedicated unreliable/unordered WebRTC data channel

#### Voice Session Management
```rust
pub struct VoiceSession {
    // Per-peer voice stream handle
    peer_id: AuthorId,
    // Opus encoder/decoder with DTX (discontinuous transmission)
    codec: OpusCodec,
    // Jitter buffer (40-120ms adaptive latency)
    jitter_buffer: JitterBuffer,
    // Voice activity detection (VAD) for bandwidth optimization
    vad: VoiceActivityDetector,
    // Metrics (packet loss, bitrate, speaking detection)
    metrics: VoiceMetricsHandle,
}
```

#### Voice Telemetry
Extend `TransportDiagnostics` with voice-specific fields:
```rust
pub struct VoiceDiagnostics {
    pub active_speakers: Vec<AuthorId>,
    pub voice_bitrate_kbps: f32,
    pub voice_packet_loss_pct: f32,
    pub voice_jitter_ms: f32,
    pub voice_latency_ms: f32,
}
```

#### Engine Integration
- Voice capture runs in parallel to frame loop via dedicated Tokio task
- Audio playback integrated with VR spatial audio system (head-relative positioning)
- Automatic gain control (AGC) and echo cancellation hooks
- Push-to-talk / voice-activated modes switchable via command

### Implementation Phases

#### Voice Foundation (Phase 5.5)
- [ ] Add `network::voice` module with Opus codec integration
- [ ] Define `VoiceSession` and `VoicePacket` data structures
- [ ] Extend WebRTC transport with unreliable/unordered voice channel
- [ ] Implement jitter buffer with adaptive latency (40-120ms)
- [ ] Add voice activity detection (VAD) for bandwidth optimization
- [ ] Extend telemetry overlay with voice diagnostics

#### Spatial Audio Integration (Phase 6.5)
- [ ] Integrate VR head tracking for spatial audio positioning
- [ ] Implement head-relative panning (HRTF or simple stereo balance)
- [ ] Add distance attenuation for positional voice
- [ ] Support mute/unmute commands via command log
- [ ] Add per-peer volume controls in editor UI

#### Voice Quality & Optimization (Phase 7.5)
- [ ] Implement forward error correction (FEC) for packet loss mitigation
- [ ] Add automatic bitrate adjustment based on network conditions
- [ ] Optimize Opus encoder settings (CBR/VBR, frame size, DTX)
- [ ] Add noise suppression filters
- [ ] Benchmark CPU usage and battery impact on Quest 3

### Test Requirements (Voice)

#### Unit Tests
- [ ] `voice::codec::opus_encode_decode_roundtrip` - Verify lossless encoding/decoding
- [ ] `voice::jitter::adaptive_latency_adjusts_correctly` - Test buffer expansion/contraction
- [ ] `voice::vad::detects_speech_vs_silence` - Validate voice activity detection
- [ ] `voice::metrics::tracks_packet_loss_and_latency` - Ensure telemetry accuracy

#### Integration Tests
- [ ] `voice_session_establishes_over_webrtc` - End-to-end voice session setup
- [ ] `voice_packets_bypass_command_queue` - Verify priority handling
- [ ] `spatial_audio_pans_with_head_rotation` - Test 3D audio positioning
- [ ] `mute_command_stops_voice_transmission` - Command integration

#### Edge Case Tests (Voice)
- [ ] `high_packet_loss_doesnt_crash_jitter_buffer` - 50%+ loss resilience
- [ ] `voice_session_survives_webrtc_reconnect` - Transport failover handling
- [ ] `concurrent_speakers_dont_overflow_mixer` - 10+ simultaneous speakers
- [ ] `voice_telemetry_survives_codec_errors` - Malformed packet handling

---

## Security & Hardening

### Completed (Phase 5)
- ✅ Nonce-based replay protection with per-author high-water marks
- ✅ Token-bucket rate limiting (burst=100, sustain=10 commands/sec)
- ✅ 64 KiB payload guards with telemetry counters
- ✅ Ed25519 signature verification for all command entries
- ✅ Role-based permissions (Viewer/Editor/Admin) with enforcement

### Pending (Phase 5+)
- [ ] Persistent nonce storage (survive process restarts)
- [ ] Dynamic rate-limit adjustment based on abuse detection
- [ ] Command audit trail (encrypted archive for compliance)
- [ ] MLS group key agreement for multi-peer sessions (post-WebRTC hardening)
- [ ] Transport-layer DDoS mitigation (connection limits, IP filtering)

### Voice-Specific Security
- [ ] Voice packet encryption (DTLS for WebRTC, QUIC built-in for QUIC transport)
- [ ] Speaker authentication (link voice streams to authenticated `AuthorId`)
- [ ] Recording consent tracking (legal compliance for multi-jurisdiction collaboration)
- [ ] Voice spam detection (excessive packet rate triggers temporary mute)

---

## Test Coverage Strategy

### Current Coverage (74 tests, 86 with `network-quic`)

#### By Focus Area
- **ECS & Scheduler:** 15 tests (entity lifecycle, stage execution, profiling, violations)
- **Command Log & Pipeline:** 16 tests (permissions, conflicts, Lamport ordering, fuzz, replay, rate limiting)
- **Replication & Schema:** 14 tests (snapshot chunking, delta diffing, manifest hashing, registry)
- **Telemetry & Metrics:** 7 tests (overlay rendering, metrics snapshots, diagnostics export, transport kind)
- **Transport (QUIC/WebRTC):** 13 tests (handshake validation, heartbeat, packet roundtrips, WebRTC fallback)
- **Editor Commands & Tools:** 7 tests (outbox lifecycle, mesh serialization, transform/tool flows)
- **VR & Rendering:** 6 tests (simulated input, OpenXR stubs, render loop smoke tests)
- **Integration Suites:** 6 tests (command pipeline, replication loopback, telemetry ingestion)

### Edge Case Enhancements (Required)

#### Transport Layer
- [ ] `quic_handshake_tolerates_out_of_order_packets` - Test packet reordering resilience
- [ ] `webrtc_transport_recovers_from_datachannel_close` - Test graceful disconnection
- [ ] `heartbeat_survives_temporary_network_partition` - Test timeout/reconnect logic
- [ ] `transport_metrics_handle_wrapping_counters` - Test u64 overflow safety
- [ ] `mixed_quic_webrtc_clients_converge_state` - Cross-transport determinism

#### Command Log
- [ ] `command_log_rejects_future_timestamps` - Prevent timestamp manipulation attacks
- [ ] `lamport_clock_handles_u64_wraparound` - Test 2^64 overflow edge case
- [ ] `rate_limiter_refills_correctly_after_long_idle` - Test token bucket edge case
- [ ] `signature_verification_rejects_malformed_keys` - Test corrupt public key handling
- [ ] `replay_tracker_handles_nonce_gaps` - Test missing nonce sequences

#### Replication
- [ ] `delta_tracker_handles_rapid_component_churn` - 1000+ insert/remove cycles
- [ ] `snapshot_chunking_handles_oversized_components` - Components > chunk limit
- [ ] `registry_deduplication_survives_concurrent_registration` - Race condition test
- [ ] `empty_delta_set_serializes_correctly` - Zero-change frame edge case

#### Telemetry
- [ ] `telemetry_overlay_truncates_excessive_history` - Memory bounds test
- [ ] `replicator_handles_serialization_failures_gracefully` - Malformed telemetry data
- [ ] `transport_diagnostics_survive_metrics_handle_drop` - Weak ref edge case

#### Voice (New)
- [ ] `opus_encoder_handles_silence_efficiently` - DTX activation test
- [ ] `jitter_buffer_recovers_from_burst_loss` - 10+ consecutive dropped packets
- [ ] `vad_doesnt_trigger_on_background_noise` - False positive suppression
- [ ] `voice_session_survives_codec_reinitialization` - Mid-session codec swap

### Fuzz Testing Expansion
- [ ] `fuzz_command_packet_deserialization` - Random binary inputs to `CommandPacket::decode`
- [ ] `fuzz_flatbuffer_schema_parsing` - Malformed FlatBuffer inputs
- [ ] `fuzz_opus_decoder` - Random audio packet inputs
- [ ] `fuzz_replication_frame_parsing` - Invalid frame kinds and payloads

### Performance Benchmarks (Criterion)
- [ ] `benchmark_command_append_throughput` - Measure commands/sec under load
- [ ] `benchmark_delta_computation_latency` - Profile delta tracker performance
- [ ] `benchmark_snapshot_encoding` - Measure serialization overhead
- [ ] `benchmark_voice_codec_latency` - Opus encode/decode round-trip time
- [ ] `benchmark_telemetry_export` - CSV/Parquet write performance

---

## Delivery Milestones (Date-Agnostic)

### Milestone 1: Security & Transport Hardening ✅
**Status:** ~90% Complete (Voice integration pending)

**Deliverables:**
- [x] Nonce-based replay protection with high-water mark tracking
- [x] Token-bucket rate limiting with configurable thresholds
- [x] 64 KiB payload guards with telemetry counters
- [x] WebRTC transport prototype with command packet delivery
- [x] Transport kind telemetry surfacing in overlay
- [ ] Persistent nonce storage (deferred to Milestone 2)
- [ ] WebRTC signaling server (in-memory prototype only)

**Exit Criteria:**
- All security tests passing (replay, rate-limit, payload guard)
- WebRTC transport test passes (`webrtc_transport_transfers_command_packets`)
- Telemetry overlay displays transport kind correctly
- Zero security vulnerabilities in audit scan

---

### Milestone 2: WebRTC Production & Voice Foundation
**Status:** Not Started

**Deliverables:**
- [ ] Replace in-memory WebRTC channel with real WebRTC data channels
- [ ] Implement signaling server (WebSocket-based, peer discovery)
- [ ] Add STUN/TURN integration for NAT traversal
- [ ] Implement `network::voice` module with Opus codec
- [ ] Add jitter buffer and voice activity detection (VAD)
- [ ] Extend telemetry with voice diagnostics
- [ ] Convergence tests: QUIC ↔ WebRTC state synchronization

**Exit Criteria:**
- Mixed QUIC/WebRTC sessions converge state deterministically
- Voice sessions establish and transmit audio packets
- Voice telemetry tracks active speakers and packet loss
- Signaling server handles 10+ concurrent peers
- STUN/TURN fallback works behind corporate NATs

---

### Milestone 3: Compression & Interest Management
**Status:** Not Started

**Deliverables:**
- [ ] Zstd compression for command/replication payloads
- [ ] Dictionary training from recorded delta samples
- [ ] Compression ratio telemetry tracking
- [ ] Spatial cell partitioning for large worlds
- [ ] Tool scope filtering (replicate mesh editor state only to editors)
- [ ] Client subscription API (`subscribe_to_region`, `unsubscribe`)
- [ ] Bandwidth benchmarks validating 50-70% reduction target

**Exit Criteria:**
- Compression reduces bandwidth by ≥50% on typical workloads
- Interest management filters ≥80% of irrelevant deltas
- Spatial partitioning scales to 10K+ entities
- Benchmark suite establishes performance baselines

---

### Milestone 4: Mesh Editor Alpha
**Status:** Not Started

**Deliverables:**
- [ ] Half-edge mesh data model with boundary tracking
- [ ] Core editing operations (vertex create, edge extrude, face subdivide)
- [ ] Undo/redo command stack with collaborative branching
- [ ] In-headset UI (tool palette, property inspector, material picker)
- [ ] glTF export with custom metadata extension
- [ ] Mesh command serialization and replication
- [ ] Conflict resolution for concurrent mesh edits

**Exit Criteria:**
- Users can create simple meshes (cube, pyramid) in VR
- Undo/redo works seamlessly across network peers
- Editor UI is readable at 1 meter distance in Quest 3
- Mesh operations maintain 90 Hz frame rate (≤11ms/frame)
- glTF export roundtrips with no data loss

---

### Milestone 5: Spatial Voice & Audio Integration
**Status:** Not Started

**Deliverables:**
- [ ] VR head tracking for spatial audio positioning
- [ ] Head-relative panning (HRTF or simple stereo balance)
- [ ] Distance attenuation for positional voice
- [ ] Mute/unmute commands via command log
- [ ] Per-peer volume controls in editor UI
- [ ] Push-to-talk mode with VR controller button mapping
- [ ] Voice quality telemetry (bitrate, latency, packet loss)

**Exit Criteria:**
- Spatial audio pans correctly with head rotation
- Distance attenuation reduces volume at >5m range
- Push-to-talk activates on controller button press
- Voice latency stays <100ms end-to-end
- Voice telemetry displays active speakers in overlay

---

### Milestone 6: OpenXR Live & Quest 3 Native
**Status:** Not Started

**Deliverables:**
- [ ] Live OpenXR action set polling (controllers, hands, eye tracking)
- [ ] wgpu swapchain → OpenXR swapchain binding
- [ ] Quest 3 APK build pipeline with symbol stripping (<200 MB target)
- [ ] Haptic feedback for tool confirmations and snap events
- [ ] Comfort settings (snap turning, vignette, guardian visualization)
- [ ] Thermal throttling mitigation (dynamic frame rate scaling)
- [ ] OVR metrics integration for performance monitoring

**Exit Criteria:**
- Engine runs natively on Quest 3 at 72 Hz baseline
- Users can edit meshes with live controller input
- APK size ≤200 MB with all core features
- Frame times stay within thermal budget (no throttling in 10-min session)
- Voice chat works over Quest 3 standalone network

---

### Milestone 7: Performance Optimization & Observability
**Status:** Not Started

**Deliverables:**
- [ ] GPU profiling hooks with per-stage breakdown
- [ ] CPU profiling via Tracy or puffin
- [ ] Memory profiling (entity counts, component storage, network buffers)
- [ ] Latency percentiles (p50, p90, p99) for all critical paths
- [ ] Prometheus metrics export for production monitoring
- [ ] OpenTelemetry traces for distributed debugging
- [ ] Crash reporting and telemetry aggregation service
- [ ] Operator runbook with incident response procedures

**Exit Criteria:**
- GPU profiling identifies render bottlenecks <1ms resolution
- Memory usage stays <2GB on Quest 3 for typical scenes
- P99 command latency <50ms in 10-peer sessions
- Prometheus dashboards visualize all critical metrics
- Crash reports auto-submit with symbolicated stack traces

---

## Technical Debt & Future Work

### Known Limitations
1. **WebRTC Signaling:** In-memory prototype only; needs production signaling server
2. **Voice Codec:** Opus integration pending; placeholder types ready
3. **Compression:** JSON serialization instead of FlatBuffers (20-30% overhead)
4. **Interest Management:** No spatial filtering yet (all entities replicate to all peers)
5. **Physics:** Rapier3D integration deferred (collision/haptics not implemented)
6. **Asset Streaming:** No CDN integration or resumable chunking

### Architectural Improvements
- Consider CRDT-based merge strategies for concurrent mesh edits (post-MVP)
- Explore GPU-accelerated boolean operations for advanced mesh tools
- Evaluate MLS group key agreement if WebRTC peer-to-peer scales poorly
- Implement procedural mesh generators (sweep, loft, revolution)
- Add analytics export for user behavior insights

---

## Development Workflow

### Build & Test
```bash
# Standard build
cargo build

# Run default tests (74 tests)
cargo test

# Run with network features (86 tests)
cargo test --features network-quic

# Run with all features
cargo test --all-features

# Format and lint
cargo fmt
cargo clippy --all-targets --all-features

# Regenerate component manifest
cargo run --bin generate_manifest
```

### Feature Flags
- `render-wgpu`: GPU rendering backend
- `vr-openxr`: OpenXR VR integration
- `network-quic`: QUIC networking + WebRTC prototype
- `physics-rapier`: Physics engine (pending)
- `voice-opus`: Voice codec integration (pending)

### CI/CD Pipeline
- [x] `cargo fmt` validation
- [x] `cargo clippy --all-targets --all-features`
- [x] `cargo test` (default features)
- [x] `cargo test --features network-quic`
- [ ] `cargo test --all-features` (add when voice/physics ready)
- [x] FlatBuffers schema validation
- [x] Component manifest freshness check
- [ ] Performance regression tests (Criterion benchmarks)
- [ ] Integration test suite with simulated multi-peer scenarios

---

## Success Metrics & KPIs

### Quality Metrics
- **Test Coverage:** ≥80% line coverage across all modules
- **Test Count:** ≥100 tests by Milestone 4 completion
- **CI Success Rate:** ≥95% green builds on main branch
- **Zero Known Security Vulnerabilities** in production builds

### Performance Metrics
- **Frame Time (Quest 3):** ≤11ms/frame (90 Hz target)
- **Command Latency (p99):** <50ms in 10-peer sessions
- **Voice Latency (p99):** <100ms end-to-end
- **Bandwidth Efficiency:** ≥50% reduction via compression + interest filtering
- **Memory Footprint (Quest 3):** <2GB for typical collaborative session

### User Experience Metrics
- **Onboarding Time:** Users create first mesh within 5 minutes
- **Collaboration Success Rate:** ≥95% of sessions establish without manual intervention
- **Voice Quality Score:** ≥4.0/5.0 (MOS score equivalent)
- **Frame Rate Stability:** <5% of frames drop below 72 Hz on Quest 3

---

## Documentation Standards

### Code Documentation
- All public APIs require doc comments with examples
- Unsafe code blocks require safety justification comments
- Performance-critical paths include complexity analysis comments
- Network protocol changes require schema version updates

### External Documentation
- Architecture decisions captured in ADR (Architecture Decision Record) format
- API changes documented in CHANGELOG.md (Keep a Changelog format)
- Breaking changes require migration guides
- Performance benchmarks published with each milestone

### Test Documentation
- Integration tests include scenario descriptions in doc comments
- Edge case tests cite specific bug reports or security advisories
- Fuzz test seeds documented for reproducibility
- Benchmark results archived in `docs/benchmarks/`

---

## Risk Register

### High Risks
1. **Quest 3 Performance Bottleneck**
   - *Mitigation:* Early GPU profiling, dynamic quality scaling, deferred features
   - *Fallback:* PCVR-first deployment if mobile targets miss perf goals

2. **WebRTC Signaling Complexity**
   - *Mitigation:* Use battle-tested libraries (webrtc-rs, matchbox), defer custom STUN/TURN
   - *Fallback:* QUIC-only for initial release; WebRTC as post-MVP feature

3. **Voice Quality Issues**
   - *Mitigation:* Benchmark Opus settings early, adaptive bitrate, packet loss concealment
   - *Fallback:* Text chat only if voice quality unacceptable

### Medium Risks
4. **Command Merge Conflicts**
   - *Mitigation:* Conservative conflict strategies, user-facing conflict resolution UI
   - *Fallback:* Last-write-wins for MVP; defer complex CRDT merges

5. **Compression Ratio Lower Than Expected**
   - *Mitigation:* Benchmark early, tune dictionary training, fallback to interest filtering
   - *Fallback:* Interest management reduces payload sizes even without compression

6. **Multiplayer Scaling Limits**
   - *Mitigation:* Start with 10-peer sessions, benchmark scalability, identify bottlenecks
   - *Fallback:* Host migration or dedicated server mode if peer-to-peer doesn't scale

---

## Appendix: Component Manifest

Current registered components (see `schemas/component_manifest.json`):
- `FrameStats`, `Transform`, `Velocity`, `EditorSelection`
- `TrackedPose`, `ControllerState`
- `TelemetrySurface`, `TelemetryReplicator`, `TelemetryComponent`
- `CommandOutbox`, `CommandTransportQueue`
- `EditorToolState`

Voice components (pending):
- `VoiceSession`, `VoiceSpeaker`, `VoiceListener`
- `SpatialAudioEmitter`, `SpatialAudioReceiver`

---

## Appendix: Network Protocol Schema

### FlatBuffers Schema (`schemas/network.fbs`)
Current message types:
- `PacketHeader` (sequence ID, schema hash, compression flag)
- `SessionHello` (protocol version, capabilities, public key, nonce)
- `SessionAcknowledge` (session ID, role, capability mask, public key, nonce)
- `Heartbeat` (timestamp, RTT, jitter)
- `MessageEnvelope` (header + body union)

Voice schema extensions (pending):
- `VoicePacket` (sequence, timestamp, opus payload, speaker ID)
- `VoiceControl` (mute, unmute, volume adjust, spatial position)

---

## Conclusion

This universal development plan consolidates the Theta Engine roadmap into a date-agnostic execution framework, expanding scope to include multiplayer voice communication and comprehensive edge-case testing. The plan balances feature delivery with quality assurance, ensuring production-ready foundations before advancing to user-facing milestones.

**Next Steps:**
1. Review and approve Milestone 2 scope (WebRTC Production & Voice Foundation)
2. Expand test suite with edge-case coverage for transport, command log, and replication layers
3. Begin voice module scaffolding in `src/network/voice.rs`
4. Update CI/CD pipeline to track edge-case test coverage metrics
5. Schedule architecture review for spatial audio integration design

**Prepared By:** GitHub Copilot (Systems & Networking)  
**Last Updated:** November 5, 2025  
**Next Review:** Upon Milestone 2 kickoff
