use theta_engine::editor::CommandOutbox;
use theta_engine::engine::Engine;

#[test]
fn engine_registers_command_outbox_component() {
    let engine = Engine::new();
    let world = engine.world();
    let outboxes = world.component_entries::<CommandOutbox>();

    assert_eq!(outboxes.len(), 1);
    let (_, outbox) = &outboxes[0];
    assert_eq!(outbox.total_batches(), 0);
    assert_eq!(outbox.total_entries(), 0);
}

#[test]
fn engine_routes_commands_into_outbox_after_run() {
    let mut engine = Engine::new();
    engine.configure_max_frames(130);
    engine.run();

    let world = engine.world();
    let outboxes = world.component_entries::<CommandOutbox>();
    assert_eq!(outboxes.len(), 1);
    let (_, outbox) = &outboxes[0];

    assert!(outbox.total_batches() >= 1);
    assert!(outbox.total_entries() >= 1);
}
