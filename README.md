# Codex VR Engine

Codex is a Rust-native VR-first game engine concept focused on high-performance rendering, ergonomic mesh authoring, and networked collaboration. This repository currently captures the architectural plan and initial scaffolding.

## Current Status
- Stage-aware scheduler executes `Startup → Simulation → Render → Editor` phases, with read-only systems fanned out via Rayon.
- Core ECS demo wires simulation transforms, editor selection state, and frame stats into the runtime loop.
- Null renderer and feature-gated `wgpu` backend render per-eye attachments and hand GPU surfaces off to the VR bridge.

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

## Immediate Roadmap
1. Land swapchain/presentation plumbing in the `wgpu` backend (per-eye surfaces mapped to VR compositor expectations).
2. Integrate OpenXR (or mock runtime) input paths to feed tracked poses/components into the ECS.
3. Stand up multiplayer skeleton (async runtime, networking service, replication events).
4. Flesh out mesh editing domain model with undo/redo stacks and serialization.

## Development Notes
- Target Rust 2024 edition.
- Favor data-oriented patterns and explicit control over memory/layout.
- Enforce modular boundaries to keep runtime/editor networking decoupled.
- Ensure code is VR-testable from Day 1 (mocked device inputs, simulation harnesses).

## Contribution Workflow
- Maintain clean git history; prefer feature branches with descriptive commits.
- Run `cargo fmt` and `cargo clippy --all-targets --all-features` before submitting PRs.
- Accompany new subsystems with focused unit/integration tests and documentation snippets.
