# Implementation Path Forward: Milestone 2 Kickoff

**Created:** November 5, 2025  
**Focus:** WebRTC Production & Multiplayer Voice Foundation  
**Target Completion:** Milestone 2 (date-agnostic per Universal Development Plan)

---

## Recent Changes Summary

### Documentation Overhaul (Completed)
1. **Created `UNIVERSAL_DEVELOPMENT_PLAN.md`**
   - Consolidated all phase plans, roadmaps, and status documents
   - Date-agnostic execution framework
   - Integrated multiplayer voice communication as first-class feature
   - Defined 7 delivery milestones with clear exit criteria
   - Comprehensive architecture reference for all subsystems

2. **Created `EDGE_CASE_TEST_PLAN.md`**
   - Systematic edge case coverage strategy (target: 120+ tests)
   - Boundary conditions, failure modes, race conditions, adversarial inputs
   - Fuzz testing targets (command packets, FlatBuffers, Opus codec)
   - Performance benchmarks (Criterion framework)
   - CI/CD pipeline stages with time budgets

3. **Updated `INDEX.md`**
   - Added Universal Development Plan as primary strategic reference
   - Added Edge Case Test Plan to testing section
   - Updated navigation guide for strategic planning roles
   - Enhanced QA engineer entry points

### Technical Foundation Review (From Semantic Search)
- **WebRTC Transport:** In-memory channel prototype functional, command packet delivery validated
- **QUIC Transport:** Production-ready, 3-stream isolation, heartbeat diagnostics
- **Command Transport Abstraction:** Unified `CommandTransport` enum supporting both transports
- **Telemetry Integration:** Transport kind tracking, metrics surfacing in overlay
- **Test Coverage:** 74 tests passing (68 unit + 6 integration), 86 with `network-quic`

---

## Milestone 2: Implementation Path

### Phase 1 - WebRTC Real Stack Integration (Priority: Critical)

#### Objectives
Replace in-memory channel prototype with production WebRTC data channels, establish signaling infrastructure, and validate NAT traversal.

#### Tasks

##### 1.1 WebRTC Library Integration
```rust
// Add dependencies to Cargo.toml
[dependencies]
webrtc = "0.9"  // webrtc-rs ecosystem
tokio-util = "0.7"  // codec for data channel framing
```

**Implementation Steps:**
1. Replace `tokio::sync::mpsc` channels in `src/network/transport.rs` with `webrtc::data_channel::DataChannel`
2. Implement `RTCPeerConnection` lifecycle management (offer/answer, ICE candidate handling)
3. Create data channel configuration (ordered/unordered, reliable/unreliable)
4. Integrate with existing `CommandTransport::WebRtc` enum variant

**Files to Modify:**
- `src/network/transport.rs` - Replace `WebRtcTransport` implementation
- `Cargo.toml` - Add webrtc-rs dependencies
- `src/network/mod.rs` - Export WebRTC error types

**Test Coverage:**
- [ ] `webrtc_peer_connection_establishes_successfully`
- [ ] `webrtc_data_channel_transfers_command_packets` (update existing test)
- [ ] `webrtc_connection_survives_ice_restart`

##### 1.2 Signaling Server Implementation
```rust
// New module: src/network/signaling.rs
pub struct SignalingServer {
    peers: HashMap<PeerId, WebSocketStream>,
    rooms: HashMap<RoomId, Vec<PeerId>>,
}

impl SignalingServer {
    pub async fn handle_offer(&mut self, from: PeerId, to: PeerId, sdp: SessionDescription);
    pub async fn handle_answer(&mut self, from: PeerId, to: PeerId, sdp: SessionDescription);
    pub async fn handle_ice_candidate(&mut self, from: PeerId, to: PeerId, candidate: IceCandidate);
}
```

**Implementation Steps:**
1. Create WebSocket server using `tokio-tungstenite` or `axum-websockets`
2. Implement peer discovery (broadcast SDP offers to room participants)
3. Implement SDP offer/answer relay (forward between peers)
4. Implement ICE candidate trickle (relay candidates as discovered)
5. Add timeout handling (offer expires after 30s if no answer)

**Files to Create:**
- `src/network/signaling.rs` - Signaling server implementation
- `tests/signaling_integration.rs` - End-to-end signaling tests

**Dependencies to Add:**
```toml
tokio-tungstenite = "0.20"  // WebSocket support
serde_json = "1.0"  // SDP/candidate serialization
```

**Test Coverage:**
- [ ] `signaling_server_relays_offer_to_target_peer`
- [ ] `signaling_server_handles_peer_disconnection_gracefully`
- [ ] `signaling_server_supports_10_concurrent_rooms`

##### 1.3 STUN/TURN Integration
```rust
// Extend WebRtcTransport configuration
pub struct WebRtcConfig {
    stun_servers: Vec<String>,  // e.g., ["stun:stun.l.google.com:19302"]
    turn_servers: Vec<TurnServer>,  // Fallback for restrictive NATs
}

pub struct TurnServer {
    urls: Vec<String>,
    username: String,
    credential: String,
}
```

**Implementation Steps:**
1. Configure `RTCConfiguration` with STUN servers (Google public STUN for testing)
2. Add TURN server configuration (use Coturn for self-hosted testing)
3. Implement credential rotation for TURN authentication
4. Add fallback logic: direct → STUN → TURN

**Files to Modify:**
- `src/network/transport.rs` - Add `WebRtcConfig`, configure ICE servers
- `tests/webrtc_nat_traversal.rs` - Test NAT scenarios

**Test Coverage:**
- [ ] `webrtc_establishes_connection_via_stun`
- [ ] `webrtc_falls_back_to_turn_behind_symmetric_nat`
- [ ] `webrtc_connection_fails_gracefully_if_all_servers_unreachable`

---

### Phase 2 - Voice Communication Foundation (Priority: High)

#### Objectives
Implement voice capture, Opus codec integration, jitter buffering, and voice activity detection.

#### Tasks

##### 2.1 Opus Codec Integration
```rust
// Add dependencies
[dependencies]
opus = "0.3"  // Opus encoder/decoder bindings
```

```rust
// New module: src/network/voice.rs
pub struct OpusCodec {
    encoder: opus::Encoder,
    decoder: opus::Decoder,
    sample_rate: u32,  // 48000 Hz (Opus native)
    frame_size: usize,  // 20ms = 960 samples @ 48kHz
}

impl OpusCodec {
    pub fn encode(&mut self, pcm: &[i16]) -> Result<Vec<u8>, OpusError>;
    pub fn decode(&mut self, opus_packet: &[u8]) -> Result<Vec<i16>, OpusError>;
}
```

**Implementation Steps:**
1. Wrap libopus C bindings with safe Rust interface
2. Configure encoder (VBR mode, 48kHz, stereo, 32kbps target bitrate)
3. Configure decoder (PLC for packet loss concealment)
4. Implement DTX (discontinuous transmission) for silence suppression

**Files to Create:**
- `src/network/voice.rs` - Voice codec and session management
- `src/network/voice/codec.rs` - Opus encoder/decoder wrapper
- `tests/voice_codec_tests.rs` - Codec round-trip tests

**Test Coverage:**
- [ ] `opus_encoder_encodes_silence_efficiently` (DTX test)
- [ ] `opus_codec_round_trip_preserves_audio_quality` (SNR ≥40dB)
- [ ] `opus_decoder_handles_packet_loss_with_plc`

##### 2.2 Voice Session Management
```rust
pub struct VoiceSession {
    peer_id: AuthorId,
    codec: OpusCodec,
    jitter_buffer: JitterBuffer,
    vad: VoiceActivityDetector,
    metrics: VoiceMetrics,
}

impl VoiceSession {
    pub async fn send_audio(&mut self, pcm: &[i16]) -> Result<(), VoiceError>;
    pub async fn receive_audio(&mut self) -> Result<Vec<i16>, VoiceError>;
}
```

**Implementation Steps:**
1. Create `VoiceSession` lifecycle (init, running, closed states)
2. Implement audio capture (use `cpal` crate for cross-platform audio I/O)
3. Implement jitter buffer (adaptive 40-120ms latency)
4. Integrate with WebRTC unreliable/unordered data channel

**Files to Create:**
- `src/network/voice/session.rs` - Voice session management
- `src/network/voice/jitter.rs` - Jitter buffer implementation
- `src/network/voice/vad.rs` - Voice activity detection

**Dependencies to Add:**
```toml
cpal = "0.15"  // Cross-platform audio I/O
```

**Test Coverage:**
- [ ] `voice_session_establishes_over_webrtc`
- [ ] `jitter_buffer_adapts_latency_to_network_jitter`
- [ ] `voice_session_survives_codec_reinitialization`

##### 2.3 Voice Activity Detection (VAD)
```rust
pub struct VoiceActivityDetector {
    energy_threshold: f32,  // dBFS threshold for speech detection
    speech_frames: usize,   // Consecutive frames above threshold
    silence_frames: usize,  // Consecutive frames below threshold
}

impl VoiceActivityDetector {
    pub fn process_frame(&mut self, pcm: &[i16]) -> VoiceActivity;
}

pub enum VoiceActivity {
    Speech,
    Silence,
}
```

**Implementation Steps:**
1. Implement energy-based VAD (RMS calculation)
2. Add hysteresis (require 3 consecutive speech/silence frames)
3. Integrate with Opus encoder (skip encoding during silence)
4. Add telemetry for VAD state changes

**Files to Create:**
- `src/network/voice/vad.rs` - VAD implementation

**Test Coverage:**
- [ ] `vad_detects_speech_above_threshold`
- [ ] `vad_ignores_background_noise_below_threshold`
- [ ] `vad_hysteresis_prevents_rapid_toggling`

##### 2.4 Voice Telemetry Integration
```rust
// Extend src/editor/telemetry.rs
pub struct VoiceDiagnostics {
    pub active_speakers: Vec<AuthorId>,
    pub voice_bitrate_kbps: f32,
    pub voice_packet_loss_pct: f32,
    pub voice_jitter_ms: f32,
    pub voice_latency_ms: f32,
}

// Add to TelemetryOverlay::text_panel
fn render_voice_stats(diagnostics: &VoiceDiagnostics) -> String;
```

**Implementation Steps:**
1. Add `VoiceDiagnostics` struct to telemetry module
2. Implement metrics tracking (bitrate, packet loss, jitter, latency)
3. Update telemetry overlay to display voice stats
4. Add test for voice telemetry rendering

**Files to Modify:**
- `src/editor/telemetry.rs` - Add voice diagnostics
- `tests/telemetry_integration.rs` - Add voice telemetry test

**Test Coverage:**
- [ ] `telemetry_overlay_displays_active_speakers`
- [ ] `voice_metrics_track_packet_loss_correctly`

---

### Phase 3 - Edge Case Test Implementation (Priority: High)

#### Objectives
Implement edge case tests from the Edge Case Test Plan, focusing on transport, command log, and replication layers.

#### Tasks

##### 3.1 Transport Edge Cases (15 tests)
**Location:** `tests/transport_edge_cases.rs` (new file)

**Priority Tests:**
- [ ] `quic_handshake_timeout_returns_error_within_5_seconds`
- [ ] `quic_heartbeat_detects_connection_death_after_3_missed`
- [ ] `quic_concurrent_sends_dont_corrupt_stream_state`
- [ ] `webrtc_ice_gathering_timeout_falls_back_to_quic`
- [ ] `webrtc_data_channel_survives_temporary_ice_disconnection`
- [ ] `mixed_quic_webrtc_clients_converge_to_identical_state`

**Implementation Notes:**
- Use `tokio::time::sleep` for timeout simulation
- Use `tokio::test` with `#[should_panic]` for crash tests
- Mock network failures with custom `AsyncRead`/`AsyncWrite` wrappers

##### 3.2 Command Log Edge Cases (10 tests)
**Location:** `tests/command_log_edge_cases.rs` (new file)

**Priority Tests:**
- [ ] `command_log_handles_lamport_clock_wraparound`
- [ ] `command_log_rejects_entries_with_future_timestamps`
- [ ] `rate_limiter_blocks_burst_of_1000_commands_in_1_second`
- [ ] `command_log_rejects_signature_tampering`
- [ ] `replay_tracker_handles_nonce_gaps_during_network_partition`

**Implementation Notes:**
- Use property-based testing with `proptest` for boundary conditions
- Mock Ed25519 signature verification failures
- Simulate network partitions with delayed message delivery

##### 3.3 Replication Edge Cases (8 tests)
**Location:** `tests/replication_edge_cases.rs` (new file)

**Priority Tests:**
- [ ] `snapshot_chunking_handles_zero_entities`
- [ ] `snapshot_chunking_handles_single_entity_larger_than_chunk_limit`
- [ ] `delta_tracker_handles_1000_component_updates_in_single_frame`
- [ ] `replication_survives_corrupted_delta_frame`
- [ ] `replication_rejects_snapshot_with_mismatched_manifest_hash`

**Implementation Notes:**
- Generate large test datasets with `rand` crate
- Inject corruption with bit-flipping
- Measure performance with `criterion` benchmarks

##### 3.4 Fuzz Testing Infrastructure
**Location:** `fuzz/` directory (new)

**Setup Steps:**
1. Install cargo-fuzz: `cargo install cargo-fuzz`
2. Initialize fuzz targets: `cargo fuzz init`
3. Create targets for:
   - `fuzz_command_packet_deserialization`
   - `fuzz_flatbuffer_schema_parsing`
   - `fuzz_opus_decoder` (deferred to Phase 2 completion)

**Execution:**
```bash
cargo +nightly fuzz run fuzz_command_packet --jobs 4 -- -max_total_time=36000  # 10 hours
```

**Test Coverage:**
- [ ] Fuzz command packet deserialization (10 hours, 0 crashes)
- [ ] Fuzz FlatBuffer schema parsing (10 hours, 0 crashes)

---

## Task Prioritization & Dependencies

### Critical Path (Milestone 2 Completion)
```
Phase 1.1 (WebRTC Library Integration)
  ↓
Phase 1.2 (Signaling Server)
  ↓
Phase 1.3 (STUN/TURN Integration)
  ↓
Phase 2.1 (Opus Codec)
  ↓
Phase 2.2 (Voice Session Management)
  ↓
Phase 2.3 (VAD)
  ↓
Phase 2.4 (Voice Telemetry)
  ↓
Phase 3 (Edge Case Tests - can run in parallel with Phase 2.3/2.4)
```

### Parallelization Opportunities
- **Phase 3.1 (Transport Edge Cases)** can run in parallel with **Phase 1.3 (STUN/TURN)**
- **Phase 3.2 (Command Log Edge Cases)** has no dependencies, can start immediately
- **Phase 3.3 (Replication Edge Cases)** has no dependencies, can start immediately
- **Phase 2.3 (VAD)** and **Phase 2.4 (Voice Telemetry)** can run in parallel

---

## Risk Mitigation

### High Risks
1. **WebRTC Complexity Underestimated**
   - Mitigation: Start with minimal feature set (data channels only, defer advanced ICE)
   - Fallback: Defer WebRTC to Milestone 3, focus on QUIC-only multiplayer for Milestone 2

2. **Opus Integration Issues**
   - Mitigation: Use well-tested `opus` crate bindings, validate with known test vectors
   - Fallback: Use simpler codec (PCM or ADPCM) for initial voice prototype

3. **Edge Case Test Time Blowout**
   - Mitigation: Prioritize critical path tests first, defer non-blocking edge cases
   - Fallback: Set time box (40 hours), accept ≥100 tests instead of 120 target

### Medium Risks
4. **Signaling Server Scalability**
   - Mitigation: Load test with 20+ concurrent peers early
   - Fallback: Use third-party signaling service (e.g., Twilio STUN/TURN)

5. **Voice Quality Below Acceptable Threshold**
   - Mitigation: Benchmark Opus settings early, adjust bitrate/complexity
   - Fallback: Text chat only for Milestone 2, defer voice to Milestone 3

---

## Success Criteria (Milestone 2 Exit)

### Functional Requirements
- [ ] WebRTC data channel establishes connection via signaling server
- [ ] STUN fallback works for symmetric NAT scenarios
- [ ] Voice sessions transmit Opus-encoded audio packets
- [ ] Jitter buffer maintains <100ms latency
- [ ] Voice activity detection reduces bandwidth during silence
- [ ] Voice telemetry displays in overlay

### Quality Requirements
- [ ] ≥100 tests passing (26+ new tests added)
- [ ] ≥80% line coverage across `network::` modules
- [ ] All fuzz targets run 10+ hours without crashes
- [ ] Zero security vulnerabilities in audit scan

### Performance Requirements
- [ ] Voice latency p99 <100ms
- [ ] Mixed QUIC/WebRTC sessions converge state in <5s
- [ ] Signaling server handles ≥10 concurrent rooms
- [ ] Voice packet loss concealment maintains intelligibility up to 10% loss

---

## Next Actions (Immediate)

### Week 1: WebRTC Foundation
1. Add webrtc-rs dependencies to Cargo.toml
2. Replace in-memory WebRTC transport with real data channels
3. Implement basic signaling server (offer/answer relay)
4. Write integration test: `webrtc_peer_connection_establishes_successfully`

### Week 2: Voice Codec & Edge Cases
5. Add opus crate dependency
6. Implement OpusCodec wrapper with encode/decode
7. Write unit tests: `opus_codec_round_trip_preserves_audio_quality`
8. Begin edge case test implementation (transport, command log, replication)

### Week 3: Voice Session & Telemetry
9. Implement VoiceSession lifecycle
10. Integrate cpal for audio capture
11. Add voice diagnostics to telemetry overlay
12. Complete edge case test suite (target: 100+ total tests)

### Week 4: Integration & Testing
13. End-to-end test: `voice_session_establishes_over_webrtc`
14. Run fuzz tests (10+ hours each)
15. Benchmark performance (voice latency, mixed transport convergence)
16. Update documentation (architecture.md, Universal Development Plan)

---

## Documentation Updates Required

Upon Milestone 2 completion:
1. Update `UNIVERSAL_DEVELOPMENT_PLAN.md` - Mark Milestone 2 complete, update test counts
2. Update `architecture.md` - Add WebRTC signaling flow diagram, voice pipeline architecture
3. Update `EDGE_CASE_TEST_PLAN.md` - Check off completed tests, add new learnings
4. Update `status_overview.md` - Reflect new test counts, phase status
5. Create `docs/voice_architecture.md` - Detailed voice subsystem design doc

---

**Prepared By:** GitHub Copilot (Implementation Planning)  
**Created:** November 5, 2025  
**Next Review:** Upon Phase 1.1 completion (WebRTC library integration)
