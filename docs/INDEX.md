# Theta Engine Documentation Index

**Last Updated:** November 5, 2025

## Quick Links

### 📋 Project Overview
- **[README](../README.md)** - Project introduction, current status, feature flags
- **[Status Overview](status_overview.md)** - Unified snapshot of phases, metrics, and active work
- **[Architecture](architecture.md)** - High-level system design, subsystem overview, design decisions
- **[Roadmap (November 2025)](roadmap_november_2025.md)** - Comprehensive development roadmap with sprint schedule
- **[Completion Summary](COMPLETION_SUMMARY.md)** - Detailed summary of all completed work (Phases 1-4)

### 📊 Current Status
- **[Phase 4 Status](phase4_status.md)** - Command Log implementation wrap-up (✅ 100% complete)
- **Current Focus:** Phase 5 kickoff (Security hardening, transport resilience, compression)
- **Active Work:** Nonce-based replay protection, WebRTC data-channel prototype, Zstd compression integration
- **Tests:** 66 passing, 0 failures

### 📖 Phase Documentation

#### Completed Phases:
- **[Phase 2 Review](archive/phase2_review.md)** - QUIC Transport & Handshake (✅ Complete)
- **[Phase 3 Plan](archive/phase3_plan.md)** - ECS Replication Pipeline Planning (historical)
- **[Phase 3 Review](archive/phase3_review.md)** - ECS Replication Pipeline (✅ Complete)
- **[Phase 1–4 Summary](archive/phase1-4_summary.md)** - Consolidated outcomes for the first four phases

#### Upcoming Phases:
- **[Phase 5 Parallel Plan](phase5_parallel_plan.md)** - Production hardening & WebRTC fallback
- **[Phase 4 Plan](phase4_plan.md)** - Reference plan for completed command log work
- **[Phase 4 Status](phase4_status.md)** - Final report for command log delivery

### 🏗️ Architecture & Design

#### Core Systems:
- **[Architecture](architecture.md)** - Overall system architecture
  - ECS design (entities, components, systems, scheduler)
  - Renderer architecture (backends, render graph, stereo pipeline)
  - VR integration (OpenXR, input abstraction, haptics)
  - Networking architecture (transport, replication, sessions)

#### Networking:
- **[Network Protocol Schema Plan](network_protocol_schema_plan.md)** - FlatBuffers schema design
  - Component ID hashing (SipHash-2-4)
  - Message catalog (SessionHello, Heartbeat, ComponentDelta, etc.)
  - Asset transfer protocol

#### Architecture Diagrams:
- **[Architecture Diagrams](architecture-diagrams.md)** - Visual system diagrams (if available)

### 📝 Schemas & Configuration
- **[Component Manifest](../schemas/component_manifest.json)** - Registered ECS components with hashes
- **[Network FlatBuffers Schema](../schemas/network.fbs)** - Network protocol message definitions

### 🔬 Testing & Quality
- **Test Coverage:** 71 tests (100% pass rate)
  - Unit tests: 53 (ECS, scheduler, command log, replication, etc.)
  - Integration tests: 6 (command pipeline, replication, telemetry)
- **CI Pipeline:** cargo fmt, clippy, tests, schema validation

### 🚀 Development Workflow

#### Getting Started:
1. Clone repository
2. Install Rust 2024 edition toolchain
3. Install FlatBuffers compiler (`flatc`) and add to PATH
4. Run `cargo test` to validate setup

#### Feature Development:
1. Enable relevant feature flags:
   - `render-wgpu` - GPU rendering backend
   - `vr-openxr` - OpenXR VR integration
   - `network-quic` - QUIC networking
   - `physics-rapier` - Physics engine (pending)
2. Run `cargo test --features network-quic` for networking tests
3. Regenerate manifest: `cargo run --bin generate_manifest`

#### Code Quality:
```bash
cargo fmt                              # Format code
cargo clippy --all-targets --all-features  # Lint
cargo test                             # Run tests
cargo test --features network-quic     # Run with networking
```

### 📈 Project Metrics (Nov 5, 2025)

#### Code Statistics:
- **Total LOC:** ~11,000 (8,500 source + 2,500 tests)
- **Modules:** ECS, Scheduler, Renderer, VR, Network, Editor, Engine
- **Dependencies:** 12 required + 5 optional (feature-gated)

#### Progress Tracking:
- **Phase 1 (Foundation):** ✅ 100% Complete
- **Phase 2 (Transport):** ✅ 100% Complete
- **Phase 3 (Replication):** ✅ 100% Complete
- **Phase 4 (Command Log):** ✅ 100% Complete
- **Phase 5 (Hardening):** � Kickoff (Nov 8-28)
- **Phase 6 (Mesh Editor):** 📋 Planned (Nov 29 - Dec 19)
- **Phase 7 (Quest 3):** 📋 Planned (Dec 20 - Jan 16, 2026)

### 🎯 Upcoming Milestones

#### November 2025:
- **Nov 7:** Phase 4 completion (command broadcasting, metrics, additional commands)
- **Nov 21:** Phase 5 completion (WebRTC, compression, interest management)

#### December 2025:
- **Dec 19:** Phase 6 completion (mesh editor alpha, undo/redo, in-headset UI)

#### January 2026:
- **Jan 16:** Phase 7 completion (Quest 3 native, OpenXR live, 72 Hz target)

### 📚 Additional Resources

#### Internal Documentation:
- Inline code documentation (rustdoc comments)
- Module-level documentation in each `mod.rs`
- Integration test examples in `tests/`

#### External References:
- [Rust Book](https://doc.rust-lang.org/book/)
- [wgpu Documentation](https://wgpu.rs/)
- [OpenXR Specification](https://www.khronos.org/openxr/)
- [FlatBuffers Guide](https://google.github.io/flatbuffers/)
- [Quinn (QUIC) Documentation](https://docs.rs/quinn/)

---

## Document Navigation Tips

### By Role:
- **New Contributors:** Start with README → Architecture → Unified Status Overview
- **Networking Engineers:** Network Protocol Schema Plan → Archived Phase 2/3 Reviews → Phase 4 Status
- **Graphics Engineers:** Architecture (Renderer section) → render/mod.rs
- **VR Engineers:** Architecture (VR section) → vr/mod.rs → vr/openxr.rs
- **Project Managers:** Roadmap → Completion Summary → Unified Status Overview

### By Topic:
- **Understanding ECS:** Architecture → ecs/mod.rs → engine/schedule.rs
- **Understanding Networking:** Network Protocol Schema Plan → archive/phase2_review.md → archive/phase3_review.md → phase4_status.md
- **Understanding Commands:** phase4_status.md → phase4_plan.md → network/command_log.rs
- **Understanding Tests:** Test files in tests/ → CI workflow

### By Phase:
- **Phase 1:** Architecture (foundational sections)
- **Phase 2:** archive/phase2_review.md → network/transport.rs
- **Phase 3:** archive/phase3_review.md → network/replication.rs
- **Phase 4:** phase4_status.md → network/command_log.rs → editor/commands.rs
- **Phase 5:** phase5_parallel_plan.md → roadmap_november_2025.md

---

## Maintenance Schedule

### Weekly Updates:
- Phase status documents (every Friday)
- Test coverage metrics (automated via CI)

### Milestone Updates:
- Roadmap (after each phase completion)
- Architecture (when major design decisions made)
- Completion Summary (monthly consolidation)

### Release Updates:
- README (version bumps, feature additions)
- CHANGELOG (if/when created for public releases)

---

**Index Maintained By:** Documentation Team  
**Next Review:** November 21, 2025 (Phase 5 midpoint)
