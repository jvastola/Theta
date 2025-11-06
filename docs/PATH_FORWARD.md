# Theta Engine: Clear Path Forward

**Date:** November 6, 2025  
**Status:** Phase 5 (Production Hardening) – Edge-Case Test Expansion

---

## Current State

### Accomplishments (November 6, 2025)
- ✅ **92 tests passing** (86 unit + 6 integration) with `network-quic` feature
- ✅ **Strict lint compliance**: `cargo clippy --all-targets --all-features -- -D warnings` passing
- ✅ **Edge-case coverage expansion**: Added 10 new tests covering:
  - Transport handshake nonce validation (QUIC/WebRTC reject empty nonces)
  - QUIC handshake timeout enforcement (<2s deadline)
  - Oversized payload handling (QUIC drops >64KiB, WebRTC errors gracefully)
  - Command log signature tampering detection
  - Rate limiting burst enforcement (100 command limit verified)
  - Replay attack prevention (stale nonce rejection confirmed)

### Technical Health
- **Build Status**: ✅ All targets compile cleanly
- **Test Suite**: ✅ 92/92 tests passing (0 failures, 0 ignored)
- **Code Quality**: ✅ Zero clippy warnings or format violations
- **Documentation**: ✅ Status overview, edge-case plan, and universal plan updated

---

## Immediate Next Steps (Nov 6-12, 2025)

### Priority 1: Complete Transport Edge Cases (Target: +6 tests)
**Goal:** Reach 98 total tests by adding heartbeat and network partition coverage

#### Tasks
1. **QUIC Heartbeat Loss Detection** (2 tests)
   ```rust
   #[test]
   fn quic_heartbeat_detects_connection_death_after_3_missed()
   #[test]
   fn quic_transport_recovers_from_temporary_network_partition()
   ```
   - **Acceptance:** Connection marked dead after 3 missed heartbeats; recovery after partition resolves
   - **Estimated Effort:** 4 hours

2. **WebRTC ICE/Signaling Timeouts** (2 tests)
   ```rust
   #[test]
   fn webrtc_ice_gathering_timeout_returns_error()
   #[test]
   fn webrtc_signaling_failure_returns_error_within_deadline()
   ```
   - **Acceptance:** Timeout errors surface within 10s; no indefinite hangs
   - **Estimated Effort:** 4 hours

3. **Transport Metrics Edge Cases** (2 tests)
   ```rust
   #[test]
   fn transport_metrics_handle_wrapping_counters()
   #[test]
   fn mixed_quic_webrtc_clients_converge_state()
   ```
   - **Acceptance:** u64 overflow handled; QUIC ↔ WebRTC sessions reach identical state
   - **Estimated Effort:** 3 hours

**Owner:** Networking team  
**Milestone:** 98 tests passing by November 12, 2025

---

### Priority 2: Replication Boundary Conditions (Target: +8 tests)
**Goal:** Reach 106 total tests by hardening delta tracker and snapshot chunking

#### Tasks
1. **Delta Tracker Stress Tests** (3 tests)
   ```rust
   #[test]
   fn delta_tracker_handles_rapid_component_churn() // 1000+ ops
   #[test]
   fn empty_delta_set_serializes_correctly()
   #[test]
   fn delta_tracker_handles_concurrent_component_reads_and_writes()
   ```
   - **Acceptance:** No data races; empty deltas serialize; high churn tracked accurately
   - **Estimated Effort:** 5 hours

2. **Snapshot Chunking Edge Cases** (3 tests)
   ```rust
   #[test]
   fn snapshot_chunking_handles_oversized_components() // > 16KB component
   #[test]
   fn snapshot_chunking_handles_zero_entities()
   #[test]
   fn component_descriptor_deduplication_handles_hash_collision()
   ```
   - **Acceptance:** Oversized components rejected or span multiple chunks; empty snapshots valid; hash collisions resolved
   - **Estimated Effort:** 4 hours

3. **Registry Concurrency** (2 tests)
   ```rust
   #[test]
   fn registry_deduplication_survives_concurrent_registration()
   #[test]
   fn replication_handles_missing_component_registration()
   ```
   - **Acceptance:** No type map corruption; missing types logged, not crashed
   - **Estimated Effort:** 3 hours

**Owner:** Replication team  
**Milestone:** 106 tests passing by November 19, 2025

---

### Priority 3: Command Log Remaining Edge Cases (Target: +5 tests)
**Goal:** Reach 111 total tests by closing command log adversarial gaps

#### Tasks
1. **Timestamp & Clock Edge Cases** (3 tests)
   ```rust
   #[test]
   fn command_log_rejects_future_timestamps()
   #[test]
   fn lamport_clock_handles_u64_wraparound()
   #[test]
   fn replay_tracker_handles_nonce_gaps()
   ```
   - **Acceptance:** Future timestamps rejected; wraparound handled; nonce gaps tracked correctly
   - **Estimated Effort:** 4 hours

2. **Security Edge Cases** (2 tests)
   ```rust
   #[test]
   fn signature_verification_rejects_malformed_keys()
   #[test]
   fn rate_limiter_refills_correctly_after_long_idle()
   ```
   - **Acceptance:** Malformed keys fail verification; token bucket refills after idle periods
   - **Estimated Effort:** 2 hours

**Owner:** Command log team  
**Milestone:** 111 tests passing by November 19, 2025

---

## Medium-Term Goals (Nov 20-30, 2025)

### Milestone: 120+ Tests & Performance Baselines

#### Tasks
1. **Fuzz Testing Infrastructure** (Estimated: 8 hours)
   - Set up `cargo-fuzz` targets for command packet, FlatBuffer, and replication frame parsing
   - Run 10-hour fuzz sessions; document crash-free runs
   - **Acceptance:** No crashes in 10+ hour fuzzing runs

2. **Performance Benchmarks** (Estimated: 12 hours)
   - Implement Criterion benchmarks for command append, delta computation, snapshot encoding
   - Establish baselines for throughput (commands/sec) and latency (p50/p90/p99)
   - **Acceptance:** Benchmark suite runs in CI; baseline metrics published in `docs/benchmarks/`

3. **Code Coverage Tooling** (Estimated: 4 hours)
   - Integrate `cargo-tarpaulin` for line/branch coverage reporting
   - Generate coverage report; identify gaps
   - **Acceptance:** ≥80% line coverage across `network::` modules

4. **Voice Module Scaffolding** (Estimated: 16 hours)
   - Create `src/network/voice.rs` module skeleton
   - Define `VoiceSession`, `VoicePacket`, `VoiceCodec` traits
   - Implement 4 voice unit tests (codec roundtrip, jitter buffer, VAD, metrics)
   - **Acceptance:** Voice module compiles; 4 tests passing

**Milestone:** 120+ tests, fuzz infrastructure live, voice scaffolding complete by November 30, 2025

---

## Long-Term Roadmap (December 2025+)

### Milestone 2: WebRTC Production & Voice Foundation
**Target:** Q1 2026

#### Key Deliverables
- Replace in-memory WebRTC channel with real `webrtc-rs` data channels
- Implement WebSocket-based signaling server (peer discovery)
- STUN/TURN integration for NAT traversal
- Opus codec integration for voice sessions
- Jitter buffer and voice activity detection (VAD)
- Voice telemetry (active speakers, packet loss, latency)

**Exit Criteria:**
- Mixed QUIC/WebRTC sessions converge state deterministically
- Voice sessions transmit audio packets with <100ms latency
- Signaling server handles 10+ concurrent peers
- STUN/TURN fallback works behind corporate NATs

---

### Milestone 3: Compression & Interest Management
**Target:** Q2 2026

#### Key Deliverables
- Zstd compression for command/replication payloads
- Dictionary training from recorded delta samples
- Spatial cell partitioning for large worlds
- Tool scope filtering (mesh editor state → editors only)
- Client subscription API (`subscribe_to_region`)

**Exit Criteria:**
- Compression reduces bandwidth by ≥50%
- Interest management filters ≥80% of irrelevant deltas
- Spatial partitioning scales to 10K+ entities

---

### Milestone 4: Mesh Editor Alpha
**Target:** Q3 2026

#### Key Deliverables
- Half-edge mesh data model with boundary tracking
- Core editing operations (vertex create, edge extrude, face subdivide)
- Undo/redo command stack with collaborative branching
- In-headset UI (tool palette, property inspector)
- glTF export with custom metadata extension

**Exit Criteria:**
- Users create simple meshes (cube, pyramid) in VR
- Undo/redo works across network peers
- Editor UI readable at 1m distance in Quest 3
- Mesh operations maintain 90 Hz frame rate

---

## Success Metrics

### Quality Gates (Enforced in CI)
- ✅ `cargo fmt` passing
- ✅ `cargo clippy --all-targets --all-features -- -D warnings` passing
- ✅ `cargo test --all-features` passing (92/92 tests)
- 🎯 Code coverage ≥80% across `network::` modules (pending tarpaulin integration)
- 🎯 Zero security vulnerabilities in audit scan

### Test Milestones
- ✅ **92 tests** (November 6, 2025)
- 🎯 **100 tests** (November 12, 2025) – Transport edge cases complete
- 🎯 **111 tests** (November 19, 2025) – Replication + command log edge cases complete
- 🎯 **120 tests** (November 30, 2025) – Fuzz infrastructure + voice scaffolding

### Performance Targets (Post-Benchmark Suite)
- Command append throughput: ≥50,000 commands/sec
- Delta computation latency: ≤5ms per frame (1000-entity world)
- Snapshot encoding: ≤10ms (1000-entity snapshot)
- Voice codec round-trip: ≤2ms (encode + decode 20ms frame)

---

## Risk Mitigation

### Known Risks & Responses

#### High Risk: Quest 3 Performance Bottleneck
- **Mitigation:** Early GPU profiling (Milestone 7), dynamic quality scaling
- **Fallback:** PCVR-first deployment if mobile targets miss perf goals
- **Owner:** Rendering team

#### Medium Risk: WebRTC Signaling Complexity
- **Mitigation:** Use `webrtc-rs` and `matchbox` libraries; defer custom STUN/TURN
- **Fallback:** QUIC-only for initial release; WebRTC post-MVP
- **Owner:** Networking team

#### Medium Risk: Voice Quality Issues
- **Mitigation:** Benchmark Opus settings early; adaptive bitrate; packet loss concealment
- **Fallback:** Text chat only if voice quality unacceptable
- **Owner:** Audio team (pending)

---

## Team Ownership & Responsibilities

### Current Active Teams
- **Networking Team:** Transport layer, WebRTC integration, edge-case tests
- **Replication Team:** Delta tracking, snapshot chunking, command log security
- **Systems Team:** ECS, scheduler, VR integration, build/CI
- **Quality Assurance:** Edge-case test plan execution, fuzz testing, coverage tracking

### Pending Team Formation
- **Audio Team:** Voice codec, spatial audio, jitter buffer (Milestone 2)
- **Editor Team:** Mesh editing, in-headset UI, tool palette (Milestone 4)
- **Observability Team:** Performance profiling, telemetry export, crash reporting (Milestone 7)

---

## Communication & Reviews

### Weekly Status Updates
- **Mondays:** Test count checkpoint, blockers discussion
- **Fridays:** Code review session, next week's priorities

### Monthly Milestones
- **November 12:** 100 tests milestone review
- **November 19:** 111 tests + edge-case plan completion
- **November 30:** 120 tests + fuzz infrastructure demo
- **December 15:** Phase 5 retrospective; Milestone 2 kickoff planning

### Documentation Cadence
- **After Each Test Batch:** Update `EDGE_CASE_TEST_PLAN.md` with ✅ checkmarks
- **Weekly:** Update `status_overview.md` with test counts and recent resolutions
- **Monthly:** Refresh `UNIVERSAL_DEVELOPMENT_PLAN.md` with milestone progress

---

## Quick Reference

### Key Commands
```bash
# Run all tests
cargo test --all-features

# Run specific edge-case test
cargo test --all-features -- quic_handshake_timeout

# Verify lint compliance
cargo clippy --all-targets --all-features -- -D warnings

# Format code
cargo fmt

# Generate coverage report (pending tarpaulin setup)
cargo tarpaulin --all-features --out Html

# Run fuzz target (pending cargo-fuzz setup)
cargo +nightly fuzz run fuzz_command_packet -- -max_total_time=3600
```

### Key Files
- Test Plan: `docs/EDGE_CASE_TEST_PLAN.md`
- Status: `docs/status_overview.md`
- Roadmap: `docs/UNIVERSAL_DEVELOPMENT_PLAN.md`
- This Document: `docs/PATH_FORWARD.md`

### Contact & Support
- **Repository:** github.com/jvastola/codex
- **Primary Maintainer:** Systems & Networking Team
- **Escalation:** File issue with `priority: high` label

---

**Prepared By:** GitHub Copilot (Systems Architecture)  
**Last Updated:** November 6, 2025  
**Next Review:** November 12, 2025 (100-test milestone checkpoint)
