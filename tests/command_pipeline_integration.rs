use theta_engine::editor::{CommandOutbox, CommandTransportQueue};
use theta_engine::engine::Engine;

#[test]
fn engine_registers_command_components() {
    let engine = Engine::new();
    let world = engine.world();
    let outboxes = world.component_entries::<CommandOutbox>();
    let transports = world.component_entries::<CommandTransportQueue>();

    assert_eq!(outboxes.len(), 1);
    let (_, outbox) = &outboxes[0];
    assert_eq!(outbox.total_batches(), 0);
    assert_eq!(outbox.total_entries(), 0);
    assert_eq!(outbox.total_packets(), 0);

    assert_eq!(transports.len(), 1);
    let (_, transport) = &transports[0];
    assert_eq!(transport.total_transmissions(), 0);
}

#[test]
fn engine_surfaces_command_packets_after_run() {
    let mut engine = Engine::new();
    engine.configure_max_frames(130);
    engine.run();

    let world = engine.world();
    let outboxes = world.component_entries::<CommandOutbox>();
    assert_eq!(outboxes.len(), 1);
    let (_, outbox) = &outboxes[0];

    assert!(outbox.total_batches() >= 1);
    assert!(outbox.total_entries() >= 1);
    assert!(outbox.total_packets() >= 1);

    let transports = world.component_entries::<CommandTransportQueue>();
    assert_eq!(transports.len(), 1);
    let (_, transport) = &transports[0];
    assert!(transport.total_transmissions() >= 1);
}
