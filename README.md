# PolyForge VR Engine

PolyForge is a Rust-native VR-first game engine & mesh authoring platform focused on high-performance rendering, ergonomic mesh tools, and networked collaboration. This repository contains architecture, scaffolding, and tests for the engine and editor runtime.

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

## Current Status
- Stage-aware scheduler runs `Startup → Simulation → Render → Editor`, profiles each stage/system, and flags read-only policy violations.
- ECS demo world simulates actor motion, editor selection state, and captures VR input samples (head + controllers).
- Renderer ships with a null backend plus a feature-gated `wgpu` backend that reuses per-eye swapchain textures and forwards GPU submissions to the VR bridge.
- VR layer provides a simulated input provider by default, with a feature-gated OpenXR provider (`vr-openxr`) that loads the runtime when available.

## Immediate Roadmap
1. Connect `wgpu` swapchain images into OpenXR session swapchains for real headset presentation.
2. Promote OpenXR input provider from simulated passthrough to live action polling and tracked pose updates.
3. Stand up multiplayer skeleton (async runtime, networking service, replication events).
4. Flesh out mesh editing domain model with undo/redo stacks and serialization.

## Feature Flags
- `render-wgpu`: enables the `wgpu` backend and GPU submission plumbing.
- `vr-openxr`: enables the OpenXR input provider (falls back to simulated input if the runtime cannot be loaded).

## Development Notes
- Target Rust 2024 edition.
- Favor data-oriented patterns and explicit control over memory/layout.
- Enforce modular boundaries to keep runtime/editor networking decoupled.
- Ensure code is VR-testable from Day 1 (mocked device inputs, simulation harnesses).

## Contribution Workflow
- Maintain clean git history; prefer feature branches with descriptive commits.
- Run `cargo fmt` and `cargo clippy --all-targets --all-features` before submitting PRs.
- Accompany new subsystems with focused unit/integration tests and documentation snippets.
