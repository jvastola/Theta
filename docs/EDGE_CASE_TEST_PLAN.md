# Edge Case Test Plan: Comprehensive Coverage Strategy

**Purpose:** Define systematic test coverage for boundary conditions, failure modes, and adversarial scenarios across all Theta Engine subsystems.

**Last Updated:** November 6, 2025  
**Current Test Count:** 103 (97 unit + 6 integration) with `network-quic`  
**Target Test Count:** 120+ upon completion of this plan  
**Recent Progress:** +10 transport edge-case tests (timeouts, oversized payloads, signature tampering, rate limiting) and +4 voice scaffolding tests (codec roundtrip, jitter buffer ordering, VAD detection, session metrics)

---

## Testing Philosophy

### Categories
1. **Boundary Conditions:** Test at limits (0, max, overflow, underflow)
2. **Failure Modes:** Network partitions, resource exhaustion, hardware failures
3. **Race Conditions:** Concurrent access, out-of-order events, timing attacks
4. **Adversarial Inputs:** Malformed data, signature tampering, replay attacks
5. **Resource Limits:** Memory pressure, CPU throttling, bandwidth constraints
6. **Interoperability:** Cross-version compatibility, mixed transports, codec variations

### Coverage Metrics
- **Line Coverage:** ≥80% across all modules
- **Branch Coverage:** ≥75% for critical paths (security, replication, command log)
- **Mutation Coverage:** ≥60% (use `cargo-mutants` to validate test effectiveness)
- **Fuzz Coverage:** 10+ hours per critical parsing function

---

## Transport Layer Edge Cases

### QUIC Transport (`src/network/transport.rs`)

#### Boundary Conditions
```rust
#[test] // ✅ IMPLEMENTED (handshake_unit_tests::session_hello_rejects_empty_nonce)
fn quic_handshake_tolerates_zero_byte_nonce() {
    // Test: Handshake with empty nonce should fail gracefully
    // Expected: Return Err(TransportError::Handshake)
}

#[test] // ✅ IMPLEMENTED (tests::quic_transport_tolerates_empty_packet_list)
fn quic_stream_handles_empty_packet_list() {
    // Test: Send empty command packet slice
    // Expected: No-op, no error
}

#[test]
fn quic_stream_handles_maximum_payload_size() {
    // Test: Send 16 MiB payload (quinn stream limit)
    // Expected: Either succeed or return clear error, no panic
}

#[test]
fn quic_connection_survives_rapid_reconnect_cycles() {
    // Test: Connect → disconnect → reconnect 100x in 10 seconds
    // Expected: No resource leaks, connection state consistent
}
```

#### Failure Modes
```rust
#[test] // ✅ IMPLEMENTED (tests::quic_handshake_timeout_returns_error_within_deadline)
fn quic_handshake_timeout_returns_error_within_5_seconds() {
    // Test: Connect to non-responsive endpoint
    // Expected: Timeout error within 5s, no indefinite hang
}

#[test] // ✅ IMPLEMENTED (tests::quic_transport_drops_oversized_command_packet)
fn quic_transport_drops_oversized_command_packet() {
    // Test: Send command packet > MAX_COMMAND_PACKET_BYTES
    // Expected: Server drops oversized frame, processes valid follow-up packet
}

#[test]
fn quic_heartbeat_detects_connection_death_after_3_missed() {
    // Test: Simulate peer crash (no heartbeat responses)
    // Expected: Connection marked dead after 3 × heartbeat interval
}

#[test]
fn quic_transport_recovers_from_temporary_network_partition() {
    // Test: Inject 30-second network blackhole, then restore
    // Expected: Connection recovers via exponential backoff reconnect
}
```

#### Race Conditions
```rust
#[test]
fn quic_concurrent_sends_dont_corrupt_stream_state() {
    // Test: 10 threads send packets concurrently on same session
    // Expected: All packets delivered in-order, no interleaving corruption
}

#[test]
fn quic_handshake_tolerates_out_of_order_packets() {
    // Test: SessionAcknowledge arrives before SessionHello completes
    // Expected: Handshake state machine handles reordering gracefully
}
```

#### Adversarial Inputs
```rust
#[test]
fn quic_rejects_handshake_with_future_protocol_version() {
    // Test: SessionHello with version = u32::MAX
    // Expected: Return Err(TransportError::UnsupportedVersion)
}

#[test]
fn quic_drops_packets_with_invalid_schema_hash() {
    // Test: PacketHeader with random schema_hash
    // Expected: Packet rejected, telemetry increments invalid_packet_count
}
```

---

### WebRTC Transport (`src/network/transport.rs`)

#### Boundary Conditions
```rust
#[test] // ✅ IMPLEMENTED (webrtc_tests::webrtc_transport_tolerates_empty_packet_list)
fn webrtc_transport_tolerates_zero_size_packet() {
    // Test: send_command_packets with empty vec
    // Expected: No-op, no error
}

#[test]
fn webrtc_data_channel_handles_ordered_channel_closure() {
    // Test: Close data channel during active packet transfer
    // Expected: In-flight packets dropped, send returns Err gracefully
}
```

#### Failure Modes
```rust
#[test] // ✅ IMPLEMENTED (webrtc_tests::webrtc_transport_drops_oversized_command_packet)
fn webrtc_transport_drops_oversized_command_packet() {
    // Test: send_command_packets with payload > SCTP MTU
    // Expected: Send error, subsequent valid packets succeed
}

#[test]
fn webrtc_signaling_failure_returns_error_within_10_seconds() {
    // Test: Simulate signaling server unreachable
    // Expected: Connection attempt fails with timeout error
}

#[test]
fn webrtc_ice_gathering_timeout_falls_back_to_quic() {
    // Test: STUN/TURN servers unreachable
    // Expected: Fallback to QUIC transport, log warning
}

#[test]
fn webrtc_data_channel_survives_temporary_ice_disconnection() {
    // Test: Simulate ICE restart due to network change
    // Expected: Connection re-establishes, no data loss for reliable channel
}
```

#### Race Conditions
```rust
#[test]
fn webrtc_concurrent_signaling_offers_dont_deadlock() {
    // Test: Two peers send SDP offers simultaneously
    // Expected: One offer wins via tie-breaker, other retries
}

#[test]
fn webrtc_transport_handles_rapid_peer_churn() {
    // Test: 10 peers join and leave within 5 seconds
    // Expected: No resource leaks, all connections cleaned up
}
```

#### Adversarial Inputs
```rust
#[test]
fn webrtc_rejects_sdp_with_excessive_ice_candidates() {
    // Test: SDP offer with 1000+ ICE candidates
    // Expected: Reject or truncate, no memory exhaustion
}

#[test]
fn webrtc_drops_packets_exceeding_sctp_mtu() {
    // Test: 65536-byte packet on unreliable channel
    // Expected: Packet dropped, telemetry tracks oversized_packet_count
}
```

---

### Mixed Transport Scenarios

```rust
#[test]
fn mixed_quic_webrtc_clients_converge_to_identical_state() {
    // Test: 3 QUIC peers + 3 WebRTC peers edit same scene
    // Expected: All peers reach same final state (hash check)
}

#[test]
fn transport_failover_preserves_command_ordering() {
    // Test: QUIC peer transitions to WebRTC mid-session
    // Expected: Command log maintains Lamport ordering, no duplicates
}

#[test]
fn transport_metrics_aggregate_correctly_across_kinds() {
    // Test: TelemetryOverlay with mix of QUIC/WebRTC peers
    // Expected: Bandwidth/latency metrics distinguish by transport kind
}
```

---

## Command Log Edge Cases (`src/network/command_log.rs`)

### Boundary Conditions

```rust
#[test]
fn command_log_handles_lamport_clock_wraparound() {
    // Test: Lamport clock reaches u64::MAX, next increment
    // Expected: Wraparound to 0 or saturate, no panic
}

#[test]
fn command_log_accepts_zero_length_payload() {
    // Test: CommandEntry with empty payload vec
    // Expected: Entry appended successfully (e.g., "heartbeat" command)
}

#[test]
fn command_log_rejects_payload_at_64_kib_plus_one_byte() {
    // Test: Payload size = 65537 bytes
    // Expected: Reject with PayloadTooLarge error, telemetry incremented
}
```

### Failure Modes

```rust
#[test]
fn command_log_survives_signature_verification_crash() {
    // Test: Malformed public key triggers Ed25519 panic
    // Expected: Catch panic, reject entry, log warning
}

#[test]
fn command_log_handles_concurrent_append_from_100_threads() {
    // Test: 100 threads append commands simultaneously
    // Expected: All entries preserved, Lamport ordering deterministic
}

#[test]
fn command_log_rejects_entries_with_future_timestamps() {
    // Test: Entry timestamp = system time + 10 minutes
    // Expected: Reject with InvalidTimestamp error (prevent time manipulation)
}
```

### Race Conditions

```rust
#[test]
fn command_log_conflict_resolution_deterministic_under_concurrent_edits() {
    // Test: 10 peers edit same entity attribute simultaneously
    // Expected: Same conflict winner across all peers (Lamport tie-break on author ID)
}

#[test]
fn replay_tracker_handles_nonce_gaps_during_network_partition() {
    // Test: Peer A receives nonces [1, 2, 5, 6] (3, 4 lost)
    // Expected: Accept 5, 6; reject 3, 4 if they arrive later (replay)
}
```

### Adversarial Inputs

```rust
#[test] // ✅ IMPLEMENTED (tests::integrate_remote_rejects_tampered_signature)
fn command_log_rejects_signature_tampering() {
    // Test: Valid entry, flip one bit in signature
    // Expected: Signature verification fails, entry rejected
}

#[test] // ✅ IMPLEMENTED (tests::integrate_remote_rate_limits_command_bursts)
fn rate_limiter_blocks_burst_of_1000_commands_in_1_second() {
    // Test: Peer sends 1000 commands within 1s (burst=100 limit)
    // Expected: First 100 accepted, rest rejected, rate_limited_count telemetry
}

#[test] // ✅ IMPLEMENTED (tests::integrate_remote_rejects_stale_entries)
fn command_log_survives_replay_attack_with_stale_nonce() {
    // Test: Replay entry with nonce < high-water mark
    // Expected: Reject with ReplayDetected error
}
}

#[test]
fn command_log_rejects_entries_from_revoked_author() {
    // Test: Append entry after author removed from session
    // Expected: Reject with UnauthorizedAuthor error
}
```

---

## Replication Edge Cases (`src/network/replication.rs`)

### Boundary Conditions

```rust
#[test]
fn snapshot_chunking_handles_zero_entities() {
    // Test: Empty world snapshot
    // Expected: Single chunk with header only, no panic
}

#[test]
fn snapshot_chunking_handles_single_entity_larger_than_chunk_limit() {
    // Test: Entity with 32 KiB component (chunk limit = 16 KiB)
    // Expected: Component spans multiple chunks or rejected with clear error
}

#[test]
fn delta_tracker_handles_1000_component_updates_in_single_frame() {
    // Test: Rapid component churn (1000 insert/update/remove ops)
    // Expected: Delta set accurate, no memory spike
}
```

### Failure Modes

```rust
#[test]
fn replication_survives_corrupted_delta_frame() {
    // Test: Delta frame with invalid frame kind byte
    // Expected: Reject frame, log warning, continue processing next frame
}

#[test]
fn component_descriptor_deduplication_handles_hash_collision() {
    // Test: Two components with identical schema hash (via forced collision)
    // Expected: Fallback to full descriptor comparison, no false dedup
}

#[test]
fn replication_handles_missing_component_registration() {
    // Test: Receive delta for unregistered component type
    // Expected: Log warning, skip component, don't crash
}
```

### Race Conditions

```rust
#[test]
fn concurrent_registry_registration_doesnt_corrupt_type_map() {
    // Test: 10 threads register different component types simultaneously
    // Expected: All types registered, no duplicates, stable type IDs
}

#[test]
fn delta_tracker_handles_concurrent_component_reads_and_writes() {
    // Test: 5 threads read deltas while 5 threads update components
    // Expected: No data races, deltas reflect consistent snapshots
}
```

### Adversarial Inputs

```rust
#[test]
fn replication_rejects_snapshot_with_mismatched_manifest_hash() {
    // Test: Snapshot with schema_hash ≠ current manifest hash
    // Expected: Reject snapshot, request re-handshake
}

#[test]
fn replication_survives_malformed_json_component_data() {
    // Test: Component payload = invalid UTF-8 or malformed JSON
    // Expected: Deserialization error, log warning, skip component
}
```

---

## Telemetry Edge Cases (`src/editor/telemetry.rs`)

### Boundary Conditions

```rust
#[test]
fn telemetry_overlay_truncates_excessive_frame_history() {
    // Test: 10,000 frames of telemetry data
    // Expected: Retain last 1000 frames, drop oldest, no memory leak
}

#[test]
fn telemetry_replicator_handles_zero_subscribers() {
    // Test: Publish telemetry with no active replication endpoints
    // Expected: No-op, no error
}
```

### Failure Modes

```rust
#[test]
fn telemetry_export_handles_csv_write_failure() {
    // Test: Simulate filesystem full during CSV export
    // Expected: Return Err, log warning, don't crash
}

#[test]
fn transport_diagnostics_survive_metrics_handle_drop() {
    // Test: Drop TransportDiagnostics while metrics_handle still exists
    // Expected: Metrics handle returns None, no panic
}
```

### Race Conditions

```rust
#[test]
fn concurrent_telemetry_snapshots_dont_corrupt_rolling_average() {
    // Test: 10 threads snapshot telemetry simultaneously
    // Expected: All snapshots consistent, rolling averages correct
}
```

### Adversarial Inputs

```rust
#[test]
fn telemetry_replicator_handles_malformed_serialization_data() {
    // Test: Inject invalid FrameTelemetry (NaN latency, negative frame time)
    // Expected: Sanitize values or reject, log warning
}
```

---

## Voice Edge Cases (`src/network/voice.rs` — Scaffolding Landed, Opus Pending)

### Boundary Conditions

```rust
#[test]
fn opus_encoder_handles_silence_frames_efficiently() {
    // Test: 100 frames of silence (all zeros)
    // Expected: DTX activates, encoded size ~10 bytes/frame
}

#[test]
fn jitter_buffer_handles_zero_latency_packets() {
    // Test: Packets arrive with identical timestamps
    // Expected: FIFO ordering, no buffer underrun
}

#[test]
fn voice_session_handles_opus_max_frame_size() {
    // Test: Encode 120ms frame (Opus maximum)
    // Expected: Successful encode/decode, latency ≤ 120ms
}
```

### Failure Modes

```rust
#[test]
fn jitter_buffer_recovers_from_burst_packet_loss() {
    // Test: Drop 10 consecutive packets
    // Expected: Buffer inserts silence frames, no underrun click
}

#[test]
fn opus_decoder_handles_corrupted_packet_gracefully() {
    // Test: Flip random bits in Opus payload
    // Expected: Decoder returns error or PLC (packet loss concealment) audio
}

#[test]
fn voice_session_survives_codec_reinitialization_mid_call() {
    // Test: Switch opus encoder from CBR to VBR during active call
    // Expected: Audio continues, brief silence during switch
}
```

### Race Conditions

```rust
#[test]
fn concurrent_voice_sessions_dont_overflow_audio_mixer() {
    // Test: 20 simultaneous speakers
    // Expected: Mixer clamps output to ±1.0, no distortion
}

#[test]
fn voice_packets_dont_block_command_packets() {
    // Test: Send 1000 voice packets/sec + 100 command packets/sec
    // Expected: Command latency unaffected (<50ms p99)
}
```

### Adversarial Inputs

```rust
#[test]
fn vad_doesnt_trigger_on_background_noise() {
    // Test: Pink noise at -40 dBFS
    // Expected: VAD returns false, no voice transmission
}

#[test]
fn voice_session_rejects_packets_from_unauthenticated_speaker() {
    // Test: VoicePacket with unknown speaker_id
    // Expected: Packet dropped, warning logged
}
```

---

## ECS & Scheduler Edge Cases

### Boundary Conditions

```rust
#[test]
fn ecs_handles_entity_id_wraparound() {
    // Test: Create u32::MAX entities, then one more
    // Expected: Generational index increments, no ID collision
}

#[test]
fn scheduler_handles_zero_duration_stage() {
    // Test: Stage completes in <1µs
    // Expected: Profiling records 0ms, no divide-by-zero
}

#[test]
fn ecs_query_handles_archetype_with_100_component_types() {
    // Test: Entity with 100 different components
    // Expected: Query succeeds, no stack overflow
}
```

### Failure Modes

```rust
#[test]
fn scheduler_detects_read_only_violation_in_parallel_stage() {
    // Test: Parallel stage writes to component (violates read-only policy)
    // Expected: Panic with clear error message
}

#[test]
fn ecs_component_registration_rejects_duplicate_type_id() {
    // Test: Register same component type twice
    // Expected: Return Err or no-op, log warning
}
```

### Race Conditions

```rust
#[test]
fn parallel_query_execution_doesnt_corrupt_component_storage() {
    // Test: 10 parallel queries read same components
    // Expected: All queries return consistent data, no data races
}
```

---

## Fuzz Testing Targets

### High-Priority Targets
```rust
#[test]
fn fuzz_command_packet_deserialization() {
    // Tool: cargo-fuzz with libFuzzer
    // Input: Random binary blobs to CommandPacket::decode
    // Goal: No crashes, all inputs return Ok or Err gracefully
    // Duration: 10 hours
}

#[test]
fn fuzz_flatbuffer_schema_parsing() {
    // Tool: cargo-fuzz
    // Input: Random binary blobs to FlatBuffer root accessors
    // Goal: No crashes, bounds checks pass
    // Duration: 10 hours
}

#[test]
fn fuzz_opus_decoder() {
    // Tool: cargo-fuzz
    // Input: Random binary blobs to opus_decode()
    // Goal: No crashes, decoder returns error or valid audio
    // Duration: 20 hours
}

#[test]
fn fuzz_replication_delta_parsing() {
    // Tool: cargo-fuzz
    // Input: Random DeltaFrame payloads
    // Goal: No crashes, invalid frames rejected cleanly
    // Duration: 10 hours
}
```

### Medium-Priority Targets
```rust
#[test]
fn fuzz_signature_verification() {
    // Input: Random public keys + signatures
    // Goal: No crashes, invalid signatures rejected
    // Duration: 5 hours
}

#[test]
fn fuzz_telemetry_csv_export() {
    // Input: Random FrameTelemetry values (NaN, Inf, negative)
    // Goal: CSV export doesn't panic, sanitizes values
    // Duration: 2 hours
}
```

---

## Performance Edge Cases (Criterion Benchmarks)

### Throughput Benchmarks
```rust
fn benchmark_command_append_throughput(c: &mut Criterion) {
    // Scenario: Append 10,000 commands to log
    // Metric: Commands/second
    // Target: ≥50,000 commands/sec
}

fn benchmark_delta_computation_latency(c: &mut Criterion) {
    // Scenario: Compute deltas for 1000-entity world
    // Metric: Microseconds per delta frame
    // Target: ≤5ms per frame
}

fn benchmark_opus_codec_round_trip(c: &mut Criterion) {
    // Scenario: Encode + decode 20ms audio frame
    // Metric: Round-trip latency
    // Target: ≤2ms
}
```

### Scaling Benchmarks
```rust
fn benchmark_replication_with_100_peers(c: &mut Criterion) {
    // Scenario: Broadcast delta to 100 subscribers
    // Metric: Microseconds per broadcast
    // Target: ≤50ms total
}

fn benchmark_voice_mixer_with_20_speakers(c: &mut Criterion) {
    // Scenario: Mix 20 concurrent voice streams
    // Metric: Audio processing time per 20ms frame
    // Target: ≤10ms
}
```

---

## Test Execution Strategy

### CI Pipeline Stages

#### Stage 1: Fast Feedback (≤2 minutes)
```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test --lib  # Unit tests only
```

#### Stage 2: Integration Tests (≤10 minutes)
```bash
cargo test --test '*'  # All integration tests
cargo test --features network-quic
```

#### Stage 3: Edge Case Suite (≤20 minutes)
```bash
cargo test --all-features -- edge_case  # All tests with "edge_case" in name
cargo test -- adversarial
cargo test -- boundary
```

#### Stage 4: Fuzz & Benchmark (Nightly)
```bash
cargo +nightly fuzz run fuzz_command_packet --jobs 4 -- -max_total_time=3600
cargo +nightly fuzz run fuzz_flatbuffer --jobs 4 -- -max_total_time=3600
cargo bench
```

### Local Development Workflow
```bash
# Before committing
cargo test --all-features
cargo clippy --all-targets --all-features

# Before PR
cargo test --all-features -- --include-ignored
cargo bench --no-run  # Verify benchmarks compile
```

---

## Coverage Tooling

### Line & Branch Coverage
```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --all-features --out Html --output-dir coverage/

# Target: ≥80% line coverage, ≥75% branch coverage
```

### Mutation Testing
```bash
# Install cargo-mutants
cargo install cargo-mutants

# Run mutation tests (catches weak tests)
cargo mutants --all-features

# Target: ≥60% mutation score
```

---

## Test Ownership & Review

### Module Ownership
- **Transport:** Network team (QUIC + WebRTC edge cases)
- **Command Log:** Replication team (security + conflict resolution)
- **Voice:** Audio team (codec + spatial audio)
- **ECS:** Core team (scheduler + component storage)
- **Telemetry:** Observability team (metrics + export)

### Review Checklist
- [ ] Test name clearly describes scenario
- [ ] Expected behavior documented in comment
- [ ] Edge case rationale cited (bug report, security advisory, spec)
- [ ] Test runs in <5 seconds (or marked `#[ignore]` for long tests)
- [ ] No flaky tests (deterministic or retries with jitter)

---

## Appendix: Known Gaps

### Missing Coverage (Prioritized)
1. **WebRTC Signaling:** No tests for SDP offer/answer exchange (pending real WebRTC stack)
2. **Voice Codec:** No Opus integration tests (pending codec implementation)
3. **Compression:** No Zstd round-trip tests (pending compression integration)
4. **Interest Management:** No spatial filtering tests (pending feature implementation)
5. **Physics:** No collision/haptics tests (pending Rapier3D integration)

### Deferred Coverage
- Cross-platform tests (Windows, Android, Quest OS)
- Hardware-specific tests (GPU driver bugs, OpenXR runtime quirks)
- Localization tests (multi-language UI, RTL text rendering)
- Accessibility tests (voice commands, screen reader compatibility)

---

## Success Criteria

### Milestone 2 Completion (WebRTC + Voice Foundation)
- [x] ≥100 tests total (current: 103)
- [ ] ≥80% line coverage across `network::` modules
- [ ] All fuzz targets run 10+ hours without crashes
- [ ] Zero security vulnerabilities in audit scan
- [ ] All edge case tests passing in CI

### Current Progress (November 6, 2025)
- [x] Transport handshake nonce validation (2 tests)
- [x] QUIC/WebRTC empty packet handling (2 tests)
- [x] QUIC handshake timeout coverage (1 test)
- [x] QUIC/WebRTC oversized payload handling (2 tests)
- [x] Command log signature tampering detection (1 test)
- [x] Command log rate limiting enforcement (1 test)
- [x] Replay attack prevention (existing test verified)
- [x] Voice scaffolding suite (codec passthrough, jitter buffer ordering, VAD detection, session metrics)

### Milestone 4 Completion (Mesh Editor Alpha)
- [ ] ≥120 tests total (20+ mesh editing edge cases)
- [ ] ≥85% line coverage across all modules
- [ ] Mutation score ≥60%
- [ ] Benchmark suite establishes performance baselines
- [ ] Edge case test suite runs in ≤20 minutes

---

**Next Actions (Priority Order):**
1. ~~Implement transport nonce validation~~ ✅ Complete
2. ~~Implement transport timeout/oversized payload tests~~ ✅ Complete
3. ~~Implement command log adversarial tests~~ ✅ Complete
4. Implement QUIC heartbeat loss detection - 2 tests
5. Implement replication boundary condition tests - 8 tests
6. Add WebRTC ICE/signaling timeout scenarios - 3 tests
7. Set up fuzz testing infrastructure (cargo-fuzz targets)
8. ~~Add voice edge case scaffolding (pending voice module creation)~~ ✅ Complete (passthrough codec, jitter buffer, RMS VAD, voice metrics + 4 unit tests)

**Prepared By:** GitHub Copilot (Quality Assurance)  
**Last Updated:** November 6, 2025  
**Next Review:** Upon completion of heartbeat/replication edge cases
