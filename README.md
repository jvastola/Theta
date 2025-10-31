# Theta Engine VR

Theta Engine is a Rust-native VR-first game engine & mesh authoring platform focused on high-performance rendering, ergonomic mesh tools, and networked collaboration. This repository contains architecture, scaffolding, and tests for the engine and editor runtime.

## Current Status (October 31, 2025)

**Phase 4 Progress:** 75% Complete (Command Log & Conflict Resolution)

### ✅ Completed Phases:
- **Phase 1 (Foundation):** ECS core, renderer, VR integration, FlatBuffers schema
- **Phase 2 (Transport):** QUIC handshake, heartbeat diagnostics, Ed25519 key exchange
- **Phase 3 (Replication):** Snapshot streaming, delta tracking, component serialization
- **Phase 4 (75%):** Command log core, signatures, outbox/queue wiring

### 🔄 In Progress (Nov 1-7):
- QUIC command broadcast & receive pipeline
- Command metrics & telemetry integration
- Transform gizmo and tool state commands

**Metrics:** 59 tests passing (53 unit + 6 integration), 0 failures, ~11,000 LOC total

## Vision
- VR-native editor inspired by PolySketch/Google Blocks with intuitive mesh creation, duplication, and undo/redo.
- Data-oriented entity-component-system (ECS) underpinning both runtime simulation and editor workflows.
- GPU-accelerated renderer prioritizing stereo VR, foveated rendering exploration, and modern APIs (Vulkan/DirectX/OpenXR).
- Multiplayer-ready foundation enabling collaborative editing and gameplay sessions.

## Architectural Overview
- `engine`: entry orchestration, scheduling, frame loop, and subsystem coordination.
- `ecs`: performant entity storage, component registration, and parallel system execution.
- `render`: GPU abstraction layer, renderer graph, and VR-specific optimizations.
- `vr`: OpenXR integration, headset/controller tracking, haptics, and VR session management.
- `editor`: mesh-authoring tools, undo/redo command stack, and PolySketch-like UX primitives.
- `network`: transport abstraction (WebRTC/QUIC), replication, and collaborative session protocols.

Each subsystem will be designed as a distinct module crate to allow modular development and unit testing. Bevy and other ecosystem crates may be leveraged selectively where they accelerate development without constraining the custom architecture.

## Current Status
- Stage-aware scheduler runs `Startup → Simulation → Render → Editor`, profiles each stage/system, and flags read-only policy violations.
- ECS demo world simulates actor motion, editor selection state, and captures VR input samples (head + controllers).
- Renderer ships with a null backend plus a feature-gated `wgpu` backend that reuses per-eye swapchain textures and forwards GPU submissions to the VR bridge.
- VR layer provides a simulated input provider by default, with a feature-gated OpenXR provider (`vr-openxr`) that loads the runtime when available.
- QUIC transport (`network-quic`) establishes Ed25519-backed handshakes with heartbeat telemetry feeding frame diagnostics.

## Immediate Roadmap (November 2025)

### Week 9 (Nov 1-7): Complete Phase 4
- Wire CommandTransportQueue into QUIC control stream for command broadcasting
- Implement receive pipeline with signature verification and ECS integration
- Add command metrics (append rate, conflicts, queue depth) to telemetry overlay
- Implement transform gizmo commands and tool state tracking
- **Target:** 67 tests passing, loopback convergence validated

### Weeks 10-12 (Nov 8-28): Phase 5 - Production Hardening
- WebRTC data channel fallback for browser-based VR peers
- Zstd compression integration (targeting 50-70% bandwidth reduction)
- Interest management implementation (spatial cells, tool scope filtering)
- 120-frame loopback convergence suite with packet drop injection
- Performance benchmarking and baseline establishment

### Weeks 13-16 (Nov 29 - Dec 19): Phase 6 - Mesh Editor Alpha
- Half-edge mesh data model with boundary tracking
- Core editing operations (vertex create, edge extrude, face subdivide)
- Undo/redo command stack with collaborative branching timelines
- In-headset UI (tool palette, property inspector, material picker)
- glTF export with custom metadata extension

### Weeks 17-20 (Dec 20 - Jan 16, 2026): Phase 7 - OpenXR Live & Quest 3
- Live OpenXR action set polling (controllers, hands, eye tracking)
- wgpu swapchain → OpenXR swapchain binding for native rendering
- Quest 3 APK build pipeline with symbol stripping (<200 MB target)
- Haptic feedback, comfort settings, thermal throttling mitigation
- Target: 72 Hz baseline on Quest 3 hardware

See `docs/roadmap_november_2025.md` for detailed sprint planning and success criteria.

## Technical Decisions (Resolved October 2025)
- **Physics Backend:** Rapier3D adopted long-term with VR-specific enhancements; custom solver deferred indefinitely.
- **Hardware Target:** Optimize for Quest 3 standalone (72-90 Hz, <200 MB APK); maintain PCVR compatibility via feature flags.
- **Network Security:** Role-based permissions (viewer/editor/admin) with Ed25519 command signing; transport security via QUIC TLS 1.3.
- **Asset Protocol:** Inline `AssetTransfer` messages via FlatBuffers schema for low-latency collaborative editing; optional CDN manifest pointers deferred to production builds.
- **Schema Hashing:** 64-bit SipHash-2-4 for component IDs with deterministic registration order enforced by declarative macros; CI validates cross-platform consistency.

## Feature Flags
- `render-wgpu`: enables the `wgpu` backend and GPU submission plumbing.
- `vr-openxr`: enables the OpenXR input provider (falls back to simulated input if the runtime cannot be loaded).
- `physics-rapier`: integrates Rapier3D physics engine with VR-optimized wrapper layers.
- `target-pcvr`: enables PCVR-specific optimizations (higher fidelity rendering, relaxed thermal constraints).
- `network-quic`: enables QUIC transport layer for multiplayer replication and collaboration.

## Development Notes
- Target Rust 2024 edition.
- Favor data-oriented patterns and explicit control over memory/layout.
- Enforce modular boundaries to keep runtime/editor networking decoupled.
- Ensure code is VR-testable from Day 1 (mocked device inputs, simulation harnesses).
- Install the FlatBuffers compiler (`flatc`) and ensure it is discoverable on the PATH for schema code generation.
- Regenerate the component manifest with `cargo run --bin generate_manifest` whenever replicated ECS components change; CI will fail if `schemas/component_manifest.json` is stale.
- Enable QUIC development flows with `cargo test --features network-quic` to validate handshakes and heartbeat diagnostics on the local loopback server.

## Contribution Workflow
- Maintain clean git history; prefer feature branches with descriptive commits.
- Run `cargo fmt` and `cargo clippy --all-targets --all-features` before submitting PRs.
- Accompany new subsystems with focused unit/integration tests and documentation snippets.
