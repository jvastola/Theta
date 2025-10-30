pub mod ecs;
pub mod editor;
pub mod engine;
pub mod network;
pub mod render;
pub mod vr;

/// Bootstraps the engine runtime. Currently a placeholder loop.
pub fn run() {
    let mut engine = engine::Engine::default();
    engine.run();
}
