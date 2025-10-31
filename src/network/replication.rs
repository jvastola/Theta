use crate::ecs::World;
use crate::network::{ComponentDescriptor, ComponentDiff, ComponentKey, DiffPayload, EntityHandle};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::to_vec;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const DEFAULT_CHUNK_LIMIT: usize = 16 * 1024;
const SNAPSHOT_ENTRY_OVERHEAD: usize = 24;

/// Marker trait for ECS components that participate in network replication.
pub trait ReplicatedComponent:
    crate::ecs::Component + Serialize + DeserializeOwned + Send + Sync + 'static
{
}

impl<T> ReplicatedComponent for T where
    T: crate::ecs::Component + Serialize + DeserializeOwned + Send + Sync + 'static
{
}

struct ComponentPacket {
    entity: crate::ecs::Entity,
    bytes: Vec<u8>,
}

struct RegistryEntry {
    key: ComponentKey,
    dump: fn(&World) -> Vec<ComponentPacket>,
}

impl RegistryEntry {
    fn dump(&self, world: &World) -> Vec<ComponentPacket> {
        (self.dump)(world)
    }
}

/// Registry describing which ECS components should be replicated across the network.
#[derive(Default)]
pub struct ReplicationRegistry {
    entries: Vec<RegistryEntry>,
    registered: HashSet<TypeId>,
}

impl ReplicationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T: ReplicatedComponent>(&mut self) {
        let type_id = TypeId::of::<T>();
        if self.registered.contains(&type_id) {
            return;
        }

        fn dump_components<T: ReplicatedComponent>(world: &World) -> Vec<ComponentPacket> {
            world
                .component_entries::<T>()
                .into_iter()
                .map(|(entity, component)| ComponentPacket {
                    entity,
                    bytes: to_vec(component).expect("component serialization"),
                })
                .collect()
        }

        self.entries.push(RegistryEntry {
            key: ComponentKey::of::<T>(),
            dump: dump_components::<T>,
        });
        self.registered.insert(type_id);
    }
}

/// Describes a serialized component instance inside a snapshot chunk.
#[derive(Debug, Clone)]
pub struct SnapshotComponent {
    pub component: ComponentKey,
    pub entity: EntityHandle,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct WorldSnapshotChunk {
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub components: Vec<SnapshotComponent>,
}

#[derive(Debug, Default, Clone)]
pub struct WorldSnapshot {
    chunks: Vec<WorldSnapshotChunk>,
}

impl WorldSnapshot {
    pub fn empty() -> Self {
        Self { chunks: Vec::new() }
    }

    pub fn chunks(&self) -> &[WorldSnapshotChunk] {
        &self.chunks
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn total_components(&self) -> usize {
        self.chunks
            .iter()
            .map(|chunk| chunk.components.len())
            .sum()
    }
}

pub struct WorldSnapshotBuilder {
    registry: Arc<ReplicationRegistry>,
    max_chunk_bytes: usize,
}

impl WorldSnapshotBuilder {
    pub fn new(registry: Arc<ReplicationRegistry>) -> Self {
        Self {
            registry,
            max_chunk_bytes: DEFAULT_CHUNK_LIMIT,
        }
    }

    pub fn with_chunk_limit(mut self, max_bytes: usize) -> Self {
        self.max_chunk_bytes = max_bytes.max(1);
        self
    }

    pub fn build(&self, world: &World) -> WorldSnapshot {
        let mut serialized = Vec::new();
        for entry in &self.registry.entries {
            for packet in entry.dump(world) {
                serialized.push(SnapshotComponent {
                    component: entry.key.clone(),
                    entity: EntityHandle::from(packet.entity),
                    bytes: packet.bytes,
                });
            }
        }

        if serialized.is_empty() {
            return WorldSnapshot::empty();
        }

        let mut chunks = Vec::new();
        let mut current_components = Vec::new();
        let mut current_size = 0usize;
        let limit = self.max_chunk_bytes;

        for component in serialized.into_iter() {
            let estimated_size = component.bytes.len() + SNAPSHOT_ENTRY_OVERHEAD;
            if !current_components.is_empty() && current_size + estimated_size > limit {
                chunks.push(WorldSnapshotChunk {
                    chunk_index: chunks.len() as u32,
                    total_chunks: 0,
                    components: current_components,
                });
                current_components = Vec::new();
                current_size = 0;
            }

            current_size += estimated_size;
            current_components.push(component);
        }

        if !current_components.is_empty() {
            chunks.push(WorldSnapshotChunk {
                chunk_index: chunks.len() as u32,
                total_chunks: 0,
                components: current_components,
            });
        }

        let total = chunks.len() as u32;
        for chunk in &mut chunks {
            chunk.total_chunks = total;
        }

        WorldSnapshot { chunks }
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct ComponentEntryKey {
    component: ComponentKey,
    entity: EntityHandle,
}

impl ComponentEntryKey {
    fn new(component: ComponentKey, entity: EntityHandle) -> Self {
        Self { component, entity }
    }
}

#[derive(Debug, Default)]
pub struct ReplicationDelta {
    pub descriptors: Vec<ComponentDescriptor>,
    pub diffs: Vec<ComponentDiff>,
}

impl ReplicationDelta {
    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }
}

pub struct DeltaTracker {
    registry: Arc<ReplicationRegistry>,
    last_state: HashMap<ComponentEntryKey, Vec<u8>>,
    advertised: HashSet<ComponentKey>,
}

impl DeltaTracker {
    pub fn new(registry: Arc<ReplicationRegistry>) -> Self {
        Self {
            registry,
            last_state: HashMap::new(),
            advertised: HashSet::new(),
        }
    }

    pub fn diff(&mut self, world: &World) -> ReplicationDelta {
        let mut diffs = Vec::new();
        let mut descriptors = Vec::new();
        let mut next_state: HashMap<ComponentEntryKey, Vec<u8>> = HashMap::new();

        for entry in &self.registry.entries {
            let packets = entry.dump(world);
            for packet in packets {
                let handle = EntityHandle::from(packet.entity);
                let key = ComponentEntryKey::new(entry.key.clone(), handle);
                let bytes = packet.bytes;

                match self.last_state.get(&key) {
                    Some(previous) if previous == &bytes => {
                        next_state.insert(key, bytes);
                    }
                    Some(_) => {
                        diffs.push(ComponentDiff {
                            entity: handle,
                            component: entry.key.clone(),
                            payload: DiffPayload::Update { bytes: bytes.clone() },
                        });
                        next_state.insert(key, bytes);
                    }
                    None => {
                        if self.advertised.insert(entry.key.clone()) {
                            descriptors.push(ComponentDescriptor {
                                key: entry.key.clone(),
                            });
                        }
                        diffs.push(ComponentDiff {
                            entity: handle,
                            component: entry.key.clone(),
                            payload: DiffPayload::Insert { bytes: bytes.clone() },
                        });
                        next_state.insert(key, bytes);
                    }
                }
            }
        }

        for (key, _) in self.last_state.iter() {
            if !next_state.contains_key(key) {
                diffs.push(ComponentDiff {
                    entity: key.entity,
                    component: key.component.clone(),
                    payload: DiffPayload::Remove,
                });
            }
        }

        self.last_state = next_state;

        ReplicationDelta { descriptors, diffs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::World;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
    struct TestComponent {
        value: u32,
    }

    fn setup_registry() -> Arc<ReplicationRegistry> {
        let mut registry = ReplicationRegistry::new();
        registry.register::<TestComponent>();
        Arc::new(registry)
    }

    fn build_world() -> World {
        let mut world = World::new();
        world.register_component::<TestComponent>();
        world
    }

    #[test]
    fn snapshot_chunking_respects_limit() {
        let registry = setup_registry();
        let mut world = build_world();

        for i in 0..5 {
            let entity = world.spawn();
            world
                .insert(entity, TestComponent { value: i })
                .expect("insert component");
        }

        let builder = WorldSnapshotBuilder::new(Arc::clone(&registry)).with_chunk_limit(80);
        let snapshot = builder.build(&world);

        assert!(!snapshot.is_empty());
        assert_eq!(snapshot.total_components(), 5);
        assert!(snapshot.chunks().len() >= 2);
        for chunk in snapshot.chunks() {
            assert_eq!(chunk.total_chunks, snapshot.chunks().len() as u32);
        }
    }

    #[test]
    fn delta_tracker_detects_insert_update_remove() {
        let registry = setup_registry();
        let mut world = build_world();
        let entity = world.spawn();
        world
            .insert(entity, TestComponent { value: 1 })
            .expect("insert component");

        let mut tracker = DeltaTracker::new(Arc::clone(&registry));

        let first = tracker.diff(&world);
        assert_eq!(first.descriptors.len(), 1);
        assert_eq!(first.diffs.len(), 1);
        match &first.diffs[0].payload {
            DiffPayload::Insert { bytes } => {
                let decoded: TestComponent = serde_json::from_slice(bytes).expect("decode");
                assert_eq!(decoded.value, 1);
            }
            other => panic!("expected insert, got {other:?}"),
        }

        if let Some(component) = world.get_mut::<TestComponent>(entity) {
            component.value = 5;
        }

        let second = tracker.diff(&world);
        assert!(second.descriptors.is_empty());
        assert_eq!(second.diffs.len(), 1);
        match &second.diffs[0].payload {
            DiffPayload::Update { bytes } => {
                let decoded: TestComponent = serde_json::from_slice(bytes).expect("decode");
                assert_eq!(decoded.value, 5);
            }
            other => panic!("expected update, got {other:?}"),
        }

        world.despawn(entity).expect("despawn");
        let third = tracker.diff(&world);
        assert!(third.descriptors.is_empty());
        assert_eq!(third.diffs.len(), 1);
        assert!(matches!(third.diffs[0].payload, DiffPayload::Remove));
    }

    #[test]
    fn empty_world_produces_empty_snapshot() {
        let registry = setup_registry();
        let world = build_world();
        let builder = WorldSnapshotBuilder::new(Arc::clone(&registry));
        let snapshot = builder.build(&world);

        assert!(snapshot.is_empty());
        assert_eq!(snapshot.total_components(), 0);
        assert_eq!(snapshot.chunks().len(), 0);
    }

    #[test]
    fn snapshot_single_component_fits_one_chunk() {
        let registry = setup_registry();
        let mut world = build_world();
        let entity = world.spawn();
        world
            .insert(entity, TestComponent { value: 42 })
            .expect("insert");

        let builder = WorldSnapshotBuilder::new(Arc::clone(&registry));
        let snapshot = builder.build(&world);

        assert_eq!(snapshot.chunks().len(), 1);
        assert_eq!(snapshot.total_components(), 1);
        assert_eq!(snapshot.chunks()[0].chunk_index, 0);
        assert_eq!(snapshot.chunks()[0].total_chunks, 1);
    }

    #[test]
    fn delta_tracker_stable_under_no_changes() {
        let registry = setup_registry();
        let mut world = build_world();
        let entity = world.spawn();
        world
            .insert(entity, TestComponent { value: 10 })
            .expect("insert");

        let mut tracker = DeltaTracker::new(Arc::clone(&registry));
        let first = tracker.diff(&world);
        assert_eq!(first.diffs.len(), 1);

        let second = tracker.diff(&world);
        assert!(second.is_empty());
        assert_eq!(second.diffs.len(), 0);

        let third = tracker.diff(&world);
        assert!(third.is_empty());
    }

    #[test]
    fn multiple_entities_tracked_independently() {
        let registry = setup_registry();
        let mut world = build_world();
        let e1 = world.spawn();
        let e2 = world.spawn();
        world.insert(e1, TestComponent { value: 1 }).expect("insert");
        world.insert(e2, TestComponent { value: 2 }).expect("insert");

        let mut tracker = DeltaTracker::new(Arc::clone(&registry));
        let first = tracker.diff(&world);
        assert_eq!(first.diffs.len(), 2);

        if let Some(c) = world.get_mut::<TestComponent>(e1) {
            c.value = 100;
        }

        let second = tracker.diff(&world);
        assert_eq!(second.diffs.len(), 1);
        assert!(matches!(second.diffs[0].payload, DiffPayload::Update { .. }));
        assert_eq!(second.diffs[0].entity.index, e1.index());
    }

    #[test]
    fn registry_deduplicates_component_registration() {
        let mut registry = ReplicationRegistry::new();
        registry.register::<TestComponent>();
        registry.register::<TestComponent>();
        registry.register::<TestComponent>();

        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.registered.len(), 1);
    }

    #[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
    struct AnotherComponent {
        name: String,
    }

    #[test]
    fn snapshot_handles_multiple_component_types() {
        let mut registry = ReplicationRegistry::new();
        registry.register::<TestComponent>();
        registry.register::<AnotherComponent>();
        let registry = Arc::new(registry);

        let mut world = World::new();
        world.register_component::<TestComponent>();
        world.register_component::<AnotherComponent>();

        let e1 = world.spawn();
        let e2 = world.spawn();
        world.insert(e1, TestComponent { value: 7 }).expect("insert");
        world
            .insert(e2, AnotherComponent {
                name: "test".into(),
            })
            .expect("insert");

        let builder = WorldSnapshotBuilder::new(Arc::clone(&registry));
        let snapshot = builder.build(&world);

        assert_eq!(snapshot.total_components(), 2);
        let component_keys: Vec<_> = snapshot
            .chunks()
            .iter()
            .flat_map(|chunk| chunk.components.iter())
            .map(|c| c.component.type_name.as_str())
            .collect();
        assert!(component_keys
            .iter()
            .any(|name| name.contains("TestComponent")));
        assert!(component_keys
            .iter()
            .any(|name| name.contains("AnotherComponent")));
    }

    #[test]
    fn delta_tracker_advertises_component_once() {
        let registry = setup_registry();
        let mut world = build_world();
        let mut tracker = DeltaTracker::new(Arc::clone(&registry));

        let e1 = world.spawn();
        world.insert(e1, TestComponent { value: 1 }).expect("insert");
        let first = tracker.diff(&world);
        assert_eq!(first.descriptors.len(), 1);

        let e2 = world.spawn();
        world.insert(e2, TestComponent { value: 2 }).expect("insert");
        let second = tracker.diff(&world);
        assert_eq!(second.descriptors.len(), 0);
    }

    #[test]
    fn chunking_enforces_minimum_one_component_per_chunk() {
        let registry = setup_registry();
        let mut world = build_world();
        let entity = world.spawn();
        world
            .insert(entity, TestComponent { value: 999 })
            .expect("insert");

        let builder = WorldSnapshotBuilder::new(Arc::clone(&registry)).with_chunk_limit(1);
        let snapshot = builder.build(&world);

        assert_eq!(snapshot.chunks().len(), 1);
        assert_eq!(snapshot.chunks()[0].components.len(), 1);
    }

    #[test]
    fn delta_tracker_handles_despawn_of_multiple_entities() {
        let registry = setup_registry();
        let mut world = build_world();
        let e1 = world.spawn();
        let e2 = world.spawn();
        let e3 = world.spawn();
        world.insert(e1, TestComponent { value: 1 }).expect("insert");
        world.insert(e2, TestComponent { value: 2 }).expect("insert");
        world.insert(e3, TestComponent { value: 3 }).expect("insert");

        let mut tracker = DeltaTracker::new(Arc::clone(&registry));
        tracker.diff(&world);

        world.despawn(e1).expect("despawn");
        world.despawn(e3).expect("despawn");

        let delta = tracker.diff(&world);
        let removes: Vec<_> = delta
            .diffs
            .iter()
            .filter(|d| matches!(d.payload, DiffPayload::Remove))
            .collect();
        assert_eq!(removes.len(), 2);
    }
}
