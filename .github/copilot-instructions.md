# Theta Engine – Copilot Instructions
- **Mission**: Ship a VR-first Rust game engine and collaborative mesh editor; keep the ECS, command pipeline, and telemetry loop deterministic across peers.
- **Entry Points**: `src/main.rs` boots `theta_engine::run()`, which builds the `engine::Engine` orchestrating all subsystems.
- **Docs**: Start with `README.md` for phase status and `docs/architecture.md` and `docs/phase5_parallel_plan.md` for subsystem intent and sprint priorities.

## Architecture Essentials
- `src/engine/mod.rs` wires the frame loop, stage scheduler, VR input providers, renderer selection, and command/telemetry components; extend systems through `Engine::add_system{,_fn,_parallel_fn}`.
- `engine/schedule` owns the stage graph (`Startup → Simulation → Render → Editor`) and captures profiling data; respect read/write metadata so parallel stages stay read-only safe.
- `ecs::World` is a slim archetype placeholder; register components before use and rely on `register_component_types!` so they show up in `schemas/component_manifest.json`.
- Command infrastructure (`editor::commands`, `engine::CommandPipeline`, `CommandOutbox`, `CommandTransportQueue`) powers undo/redo and replication; when emitting commands, set the correct `CommandScope` and keep payloads `serde_json` friendly.
- Telemetry lives in `editor::telemetry`; use `FrameTelemetry` to surface frame stats, and publish through `TelemetryReplicator` so network transports can stream diagnostics.

## Networking & Schema
- QUIC integration (feature `network-quic`) depends on `network/transport`, `command_log`, `replication`, and `voice`; tests expect Ed25519 signatures, Lamport clocks, and per-author metrics to remain consistent.
- **Runtime Management:** All async networking tasks share a Tokio runtime (`Arc<TokioRuntime>`). Engine manages lifecycle via `ensure_network_runtime()` (lazy init) and `set_network_runtime()` (test injection). When calling async transport methods, capture the runtime **before** borrowing transport/pipeline to avoid borrow conflicts (see command dispatch in `update_frame_diagnostics`).
- **Voice Pipeline:** `network::voice` provides Opus codec, jitter buffer, VAD, and CPAL playback. Engine synthesizes test audio via `synthesize_and_send_voice()` and drains incoming packets via `drain_incoming_voice()` each frame. Voice metrics (`VoiceDiagnostics`) surface in `WebRtcTelemetry` and are updated by both send/receive paths.
- Schemas use FlatBuffers (`schemas/network.fbs`); regenerate with `flatc` when the schema changes and keep the component manifest in sync via `cargo run --bin generate_manifest [optional/path]`.
- Stable component IDs come from SipHash seeds in `network/schema.rs`; adding components without registering breaks determinism and blocks replication tests.

## Rendering & VR
- Renderer picks backends via `render::BackendKind`; null backend is default, `render-wgpu` enables `render/wgpu_backend.rs`. Guard GPU-specific code with the same features.
- VR input defaults to `SimulatedInputProvider`; feature `vr-openxr` swaps in `vr/openxr::OpenXrInputProvider`. Always fall back gracefully and log hardware failures.

## Workflows & Testing
- Build with `cargo build`; run the frame loop using the VS Code task (`cargo run`). `Engine::configure_max_frames` caps iterations (default 3) to keep tests deterministic.
- Core suites live under `tests/`: `command_pipeline_integration.rs`, `replication_integration.rs`, `telemetry_integration.rs`, and `voice_integration.rs`. Use `cargo test` and rerun with relevant features (`cargo test --all-targets --features network-quic,render-wgpu` when touching transport or GPU paths).
- Voice integration tests require shared runtime injection: call `engine.set_network_runtime(runtime.clone())` before attaching transports to ensure async operations execute on the test's runtime.
- Keep formatting/lints clean using `cargo fmt` and `cargo clippy --all-targets --all-features` before posting changes.

## Collaboration Tips
- New systems should register components in `engine::Engine::register_core_systems` or a dedicated setup stage; expose critical metrics via telemetry if they affect frame health.
- Command packets should flow outbox → transport queue → pipeline; if you bypass a layer, update integration tests and metrics counters in `editor::CommandTransportQueue`.
- When expanding replication, adjust both `ReplicationRegistry` (type registration) and `DeltaTracker` expectations; integration tests assert chunking and diff semantics.
- Update the relevant docs in `docs/` (e.g., `status_overview.md`, sprint plans) when changing subsystem behavior; reviewers expect documentation alongside code.
