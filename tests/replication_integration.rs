use theta_engine::ecs::World;
use theta_engine::network::replication::{
    DeltaTracker, ReplicationRegistry, WorldSnapshotBuilder,
};
use theta_engine::network::DiffPayload;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Position {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Velocity {
    dx: f32,
    dy: f32,
    dz: f32,
}

#[test]
fn full_snapshot_to_delta_convergence() {
    let mut registry = ReplicationRegistry::new();
    registry.register::<Position>();
    registry.register::<Velocity>();
    let registry = Arc::new(registry);

    // Host world
    let mut host_world = World::new();
    host_world.register_component::<Position>();
    host_world.register_component::<Velocity>();

    // Client world
    let mut client_world = World::new();
    client_world.register_component::<Position>();
    client_world.register_component::<Velocity>();

    // Create entities on host
    let host_e1 = host_world.spawn();
    let host_e2 = host_world.spawn();
    host_world
        .insert(
            host_e1,
            Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
        )
        .expect("insert");
    host_world
        .insert(
            host_e2,
            Velocity {
                dx: 0.1,
                dy: 0.2,
                dz: 0.3,
            },
        )
        .expect("insert");

    // Build snapshot on host
    let builder = WorldSnapshotBuilder::new(Arc::clone(&registry));
    let snapshot = builder.build(&host_world);

    assert_eq!(snapshot.total_components(), 2);

    // Simulate client applying snapshot
    for chunk in snapshot.chunks() {
        for component in &chunk.components {
            let entity = client_world.spawn();
            // In real implementation, would deserialize and apply to ECS
            // For this test, verify serialized bytes match
            if component.component.type_name.contains("Position") {
                let pos: Position =
                    serde_json::from_slice(&component.bytes).expect("deserialize position");
                assert_eq!(pos.x, 1.0);
            } else if component.component.type_name.contains("Velocity") {
                let vel: Velocity =
                    serde_json::from_slice(&component.bytes).expect("deserialize velocity");
                assert_eq!(vel.dx, 0.1);
            }
        }
    }

    // Now simulate incremental updates via delta tracker
    let mut tracker = DeltaTracker::new(Arc::clone(&registry));
    let initial_delta = tracker.diff(&host_world);
    assert_eq!(initial_delta.diffs.len(), 2);
    assert_eq!(initial_delta.descriptors.len(), 2);

    // Modify host world
    if let Some(pos) = host_world.get_mut::<Position>(host_e1) {
        pos.x = 10.0;
    }

    let update_delta = tracker.diff(&host_world);
    assert_eq!(update_delta.diffs.len(), 1);
    assert!(update_delta.descriptors.is_empty());
    assert!(matches!(
        update_delta.diffs[0].payload,
        DiffPayload::Update { .. }
    ));
}

#[test]
fn large_world_snapshot_chunking() {
    let mut registry = ReplicationRegistry::new();
    registry.register::<Position>();
    let registry = Arc::new(registry);

    let mut world = World::new();
    world.register_component::<Position>();

    // Create 100 entities
    for i in 0..100 {
        let entity = world.spawn();
        world
            .insert(
                entity,
                Position {
                    x: i as f32,
                    y: i as f32 * 2.0,
                    z: i as f32 * 3.0,
                },
            )
            .expect("insert");
    }

    // Small chunk limit forces multiple chunks
    let builder = WorldSnapshotBuilder::new(Arc::clone(&registry)).with_chunk_limit(512);
    let snapshot = builder.build(&world);

    assert_eq!(snapshot.total_components(), 100);
    assert!(snapshot.chunks().len() > 1);

    // Verify chunk indices are sequential
    for (i, chunk) in snapshot.chunks().iter().enumerate() {
        assert_eq!(chunk.chunk_index, i as u32);
        assert_eq!(chunk.total_chunks, snapshot.chunks().len() as u32);
    }

    // Verify all components are accounted for
    let total_in_chunks: usize = snapshot
        .chunks()
        .iter()
        .map(|chunk| chunk.components.len())
        .sum();
    assert_eq!(total_in_chunks, 100);
}

#[test]
fn delta_tracker_multi_frame_consistency() {
    let mut registry = ReplicationRegistry::new();
    registry.register::<Position>();
    registry.register::<Velocity>();
    let registry = Arc::new(registry);

    let mut world = World::new();
    world.register_component::<Position>();
    world.register_component::<Velocity>();

    let mut tracker = DeltaTracker::new(Arc::clone(&registry));

    // Frame 1: spawn entities
    let e1 = world.spawn();
    let e2 = world.spawn();
    world
        .insert(e1, Position {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        })
        .expect("insert");
    world
        .insert(e2, Velocity {
            dx: 1.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("insert");

    let frame1 = tracker.diff(&world);
    assert_eq!(frame1.diffs.len(), 2);
    assert!(frame1.diffs.iter().all(|d| matches!(
        d.payload,
        DiffPayload::Insert { .. }
    )));

    // Frame 2: no changes
    let frame2 = tracker.diff(&world);
    assert!(frame2.is_empty());

    // Frame 3: update one entity
    if let Some(pos) = world.get_mut::<Position>(e1) {
        pos.x = 5.0;
    }
    let frame3 = tracker.diff(&world);
    assert_eq!(frame3.diffs.len(), 1);
    assert!(matches!(
        frame3.diffs[0].payload,
        DiffPayload::Update { .. }
    ));

    // Frame 4: add component to existing entity
    world
        .insert(e1, Velocity {
            dx: 2.0,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("insert");
    let frame4 = tracker.diff(&world);
    assert_eq!(frame4.diffs.len(), 1);
    assert!(matches!(
        frame4.diffs[0].payload,
        DiffPayload::Insert { .. }
    ));

    // Frame 5: despawn entity
    world.despawn(e2).expect("despawn");
    let frame5 = tracker.diff(&world);
    assert_eq!(frame5.diffs.len(), 1);
    assert!(matches!(frame5.diffs[0].payload, DiffPayload::Remove));
}
