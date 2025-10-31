use crate::ecs::Entity;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EntityHandle {
    pub index: u32,
    pub generation: u32,
}

impl From<Entity> for EntityHandle {
    fn from(entity: Entity) -> Self {
        Self {
            index: entity.index(),
            generation: entity.generation(),
        }
    }
}

impl From<EntityHandle> for Entity {
    fn from(handle: EntityHandle) -> Self {
        Entity::new(handle.index, handle.generation)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ComponentKey {
    pub type_name: String,
    pub type_hash: u64,
}

impl ComponentKey {
    pub fn of<T: 'static>() -> Self {
        Self::from_type_id(TypeId::of::<T>(), std::any::type_name::<T>())
    }

    pub fn from_type_id(_type_id: TypeId, type_name: &'static str) -> Self {
        Self {
            type_name: type_name.to_string(),
            type_hash: hash_type_name(type_name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffPayload {
    Insert { bytes: Vec<u8> },
    Update { bytes: Vec<u8> },
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentDiff {
    pub entity: EntityHandle,
    pub component: ComponentKey,
    pub payload: DiffPayload,
}

impl ComponentDiff {
    pub fn insert<T: 'static>(entity: Entity, bytes: Vec<u8>) -> Self {
        Self {
            entity: entity.into(),
            component: ComponentKey::of::<T>(),
            payload: DiffPayload::Insert { bytes },
        }
    }

    pub fn update<T: 'static>(entity: Entity, bytes: Vec<u8>) -> Self {
        Self {
            entity: entity.into(),
            component: ComponentKey::of::<T>(),
            payload: DiffPayload::Update { bytes },
        }
    }

    pub fn remove<T: 'static>(entity: Entity) -> Self {
        Self {
            entity: entity.into(),
            component: ComponentKey::of::<T>(),
            payload: DiffPayload::Remove,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ChangeSet {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub diffs: Vec<ComponentDiff>,
}

impl ChangeSet {
    pub fn new(sequence: u64, timestamp_ms: u64, diffs: Vec<ComponentDiff>) -> Self {
        Self {
            sequence,
            timestamp_ms,
            diffs,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }
}

pub struct NetworkSession {
    sequence: u64,
}

impl NetworkSession {
    pub fn new() -> Self {
        Self { sequence: 0 }
    }

    pub fn connect() -> Self {
        Self::new()
    }

    pub fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    pub fn craft_change_set(&mut self, diffs: Vec<ComponentDiff>) -> ChangeSet {
        let sequence = self.next_sequence();
        let timestamp_ms = current_time_millis();
        ChangeSet::new(sequence, timestamp_ms, diffs)
    }
}

fn hash_type_name(type_name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    type_name.hash(&mut hasher);
    hasher.finish()
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Position(f32, f32, f32);

    #[test]
    fn component_key_is_stable_for_type() {
        let key_a = ComponentKey::of::<Position>();
        let key_b = ComponentKey::of::<Position>();
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn change_set_serializes() {
        let mut session = NetworkSession::new();
        let entity = Entity::new(1, 0);
        let diff = ComponentDiff::insert::<Position>(entity, vec![1, 2, 3]);
        let change_set = session.craft_change_set(vec![diff]);

        let output = serde_json::to_string(&change_set).expect("serialization should succeed");
        assert!(output.contains("sequence"));
    }
}
