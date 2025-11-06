# Phase 5 Parallel Work Plan

**Date:** October 31, 2025  
**Author:** GitHub Copilot (Systems & Networking)  
**Audience:** Networking, Systems, Editor, Platform/Infra teams

## Context
- Phase 4 (Command Log & Conflict Resolution) is complete. Metrics, telemetry, and extended editor command vocabulary are merged and validated (71 tests passing).
- Phase 5 objectives focus on production hardening: securing the command pipeline, strengthening transports, adding performance instrumentation, and preparing documentation for broader team onboarding.
- The roadmap accelerates into overlapping workstreams so that Networking/Security, Systems/Telemetry, Editor Tools, and Platform/Infra can execute in parallel while sharing common integration checkpoints.

## Architectural Priorities
1. **Secure Command Replication:** Nonce-based replay protection, rate limiting, and payload validation ensure trust boundaries as collaboration scales beyond trusted LAN peers.
2. **Resilient Transports:** QUIC remains primary, but WebRTC fallback and convergence tests are required for browser clients and future public betas.
3. **Deterministic Performance Envelope:** Compression, benchmarking, and telemetry exports establish repeatable baselines before mesh tooling increases traffic.
4. **Operator Readiness:** Documentation and schema specs need to keep pace so new teams (tools, UX, QA) can integrate without reverse-engineering protocols.

## Parallel Workstreams

| Workstream | Team | Scope | Dependencies | Sprint Deliverables |
|------------|------|-------|--------------|---------------------|
| **Security & Integrity** | Networking & Security | Nonce replay protection, per-author rate limiting, payload size guards, signature rejection telemetry | Phase 4 command pipeline, QUIC transport | `CommandPacket` nonce field + verifier ✅, rate limiter with configurable thresholds ✅, 64 KiB guard + telemetry ✅ |
| **Transport Resilience** | Networking & Security + Platform | WebRTC data-channel fallback, multi-protocol convergence suite, soak test harness | Security workstream for shared metrics; CI infra | Browser peer prototype, convergence test covering QUIC/WebRTC, nightly soak task in CI |
| **Compression & Benchmarking** | Systems & Telemetry | Zstd compression for commands, telemetry export to CSV/Parquet, nightly perf benchmarks | Security (payload guard) to finalize envelope | Compression toggle w/ metrics, benchmark harness w/ baseline report, telemetry artifact publishing |
| **Editor Protocol & Tooling** | Editor Tools | Publish command protocol schema, update tool docs, plan Phase 6 mesh data flow | Phase 4 command vocab, Systems telemetry fields | `editor_command_schema.json` + Markdown spec, PolySketch tool matrix, mesh command backlog grooming |
| **DevOps & Documentation** | Platform/Infra | Update architecture diagrams, operator runbooks, task automation | Inputs from all workstreams | Revamped docs/architecture diagrams, updated runbooks, consolidated Phase 5 status dashboard |

## Cross-Team To-Do List

### Networking & Security
- [x] Add `nonce: u64` to `CommandPacket`; persist high-water mark per author; reject stale values.
- [x] Integrate token-bucket limiter (burst=100, sustain=10 commands/sec) with telemetry alerts and configurable thresholds.
- [x] Enforce 64 KiB payload cap in transport receive; emit diagnostics and drop oversize packets.
- [ ] Extend tests: replay rejection, limiter saturation, oversize payload handling.
- [x] Introduce `CommandLogConfig` security defaults (rate limiter + replay tracker scaffolding) with unit coverage.

#### Test Suite Enhancements (Networking & Security)
- [ ] **Replay Protection:** Add unit and integration tests for nonce-based replay rejection, including edge cases (wraparound, duplicate, out-of-order nonces).
- [ ] **Rate Limiting:** Simulate burst and sustained command floods; assert limiter triggers, telemetry alerts, and correct rejection behavior.
- [ ] **Payload Guard:** Fuzz oversized and malformed payloads; verify diagnostics, safe drops, and no panics.
- [ ] **Signature Verification:** Negative-path tests for invalid, missing, or tampered signatures.
- [ ] **CI Enforcement:** All new security features require passing tests before merge (CI gate).

### Systems & Telemetry
- [ ] Implement Zstd compression adapter with heuristics for small payload bypass.
- [ ] Capture compression ratio, compression/decompression latency in telemetry overlay.
- [ ] Stand up nightly perf benchmark (command append flood, remote apply, telemetry export).
- [ ] Ship telemetry export job writing CSV into `target/metrics/` for CI artifact capture.

#### Test Suite Enhancements (Systems & Telemetry)
- [ ] **Compression:** Add regression tests for compression ratio, latency, and bypass threshold logic. Validate decompression correctness and error handling.
- [ ] **Benchmarking:** Automate performance benchmarks (command throughput, latency, memory) with trend tracking and failure alerts.
- [ ] **Telemetry Export:** Test CSV/Parquet export jobs for schema correctness and artifact presence in CI.
- [ ] **Metrics Coverage:** Ensure all new telemetry fields have corresponding test assertions.

### Editor Tools
- [ ] Author `docs/editor_command_protocol.md` (schema, payload examples, conflict strategies).
- [ ] Produce tool behavior matrix linking commands to ECS components for QA sign-off.
- [ ] Define PolySketch mesh pipeline requirements (half-edge data shape, undo stack expectations) ahead of Phase 6.
- [ ] Coordinate with Systems team on telemetry hooks needed for tool UX (queue depth warnings, conflict prompts).

#### Test Suite Enhancements (Editor Tools)
- [ ] **Protocol Schema:** Validate all documented command payloads with roundtrip (serialize/deserialize) tests.
- [ ] **Tool Matrix:** Add integration tests for each tool command, asserting ECS state changes and telemetry hooks.
- [ ] **Mesh Pipeline:** Skeleton tests for mesh command payloads, undo/redo stack, and error handling.
- [ ] **QA Automation:** Link tool matrix to automated test coverage reports for sign-off.

### Platform / Infra
- [ ] Update architecture diagrams (transport, telemetry, command flow) to reflect Phase 5 changes.
- [ ] Add CI task for nightly soak tests (QUIC + WebRTC) with trend reporting.
- [ ] Publish operator runbook detailing mitigation steps for rate-limit triggers and replay alarms.
- [ ] Ensure documentation site aggregates new specs (command protocol, telemetry reference, roadmap).

#### Test Suite Enhancements (Platform / Infra)
- [ ] **CI & Soak Tests:** Automate nightly soak tests for all supported transports (QUIC, WebRTC); report failures and trends to dashboard.
- [ ] **Operator Runbook:** Validate all documented mitigation steps with scripted incident simulations.
- [ ] **Documentation Coverage:** Add CI check to ensure all new features have corresponding doc/test references before merge.

## Updated Roadmap (Oct 31 Snapshot)

| Dates | Phase | Focus | Parallel Contributors |
|-------|-------|-------|-----------------------|
| Nov 1-14 | Phase 5 Sprint A | Security & Transport bedrock (nonce, rate limiting, WebRTC proto) | Networking/Security, Platform |
| Nov 15-28 | Phase 5 Sprint B | Compression, benchmarking, operator docs | Systems/Telemetry, Platform |
| Nov 29-Dec 19 | Phase 6 Sprint A | Mesh editor alpha foundations (half-edge model, tool hooks) | Editor Tools, Systems |
| Dec 20-Jan 16 | Phase 6 Sprint B | VR UX polish, OpenXR live integration, haptics | VR/Rendering, Editor |
| Jan 17-Feb 14 | Phase 7 | Quest 3 native build, performance tuning | Platform, Rendering |

## Coordination & Checkpoints
- **Weekly Sync (Wednesdays):** Cross-team status review; highlight blocking dependencies.
- **Integration Gates:**
  - Security features merged before WebRTC fallback leaves prototype (ensures telemetry + clamping ready).
  - Compression changes gated on security payload size checks to avoid double-handling edge cases.
  - Editor protocol documentation finalized before Phase 6 backlog grooming session.
- **Definition of Done Updates:** All new features require telemetry hooks, documentation references, and automated tests before close-out.

## Risks & Mitigations
| Risk | Impact | Mitigation |
|------|--------|------------|
| WebRTC prototype slips due to unfamiliar APIs | Browser client milestone delay | Pair Networking with Platform engineer experienced in WebRTC; begin with loopback harness before UI integration |
| Rate limiting false positives in bursty collaborative sessions | User frustration, dropped commands | Configurable thresholds via runtime config; telemetry to monitor real usage before tightening caps |
| Compression regressions on small payloads | Increased latency instead of savings | Heuristic to skip compression for small payloads (threshold configurable; see Systems & Telemetry task), with regression tests capturing latency across payload sizes |
| Documentation lag | New teams blocked on onboarding | Platform team owns doc publish checklist, weekly reviews ensure updates land with feature merges |

## Next Steps
1. Confirm team leads for each workstream (owners to ACK in upcoming sync).
2. Create tracking issues in project board referencing tasks above.
3. Schedule initial benchmarking and WebRTC prototype spikes for Sprint A kick-off (Nov 1).
4. Publish updated docs (telemetry reference – done, command protocol schema – in progress) to shared knowledge base.

---

For questions or updates, contact the Phase 5 coordinator (`@systems-lead`) or raise items in the #theta-phase5 channel.
