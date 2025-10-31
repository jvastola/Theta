use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;

/// Handle referencing an entity within the ECS world.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }

    pub const fn to_raw(self) -> (u32, u32) {
        (self.index, self.generation)
    }
}

#[derive(Default)]
struct EntityRecord {
    generation: u32,
    alive: bool,
}

/// Marker trait for types that can be stored as components.
pub trait Component: Any + Send + Sync {}

impl<T: Any + Send + Sync> Component for T {}

trait AnyStorage: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn remove(&mut self, entity: Entity);
}

struct ComponentMap<T: Component> {
    entries: HashMap<Entity, T>,
}

impl<T: Component> ComponentMap<T> {
    fn insert(&mut self, entity: Entity, value: T) -> Option<T> {
        self.entries.insert(entity, value)
    }

    fn get(&self, entity: Entity) -> Option<&T> {
        self.entries.get(&entity)
    }

    fn get_mut(&mut self, entity: Entity) -> Option<&mut T> {
        self.entries.get_mut(&entity)
    }

    fn iter(&self) -> impl Iterator<Item = (&Entity, &T)> {
        self.entries.iter()
    }
}

impl<T: Component> Default for ComponentMap<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<T: Component> AnyStorage for ComponentMap<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove(&mut self, entity: Entity) {
        self.entries.remove(&entity);
    }
}

/// Errors returned by ECS operations.
#[derive(Debug)]
pub enum EcsError {
    NoSuchEntity(Entity),
}

impl fmt::Display for EcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EcsError::NoSuchEntity(entity) => {
                write!(f, "entity {:?} is not alive in this world", entity)
            }
        }
    }
}

impl std::error::Error for EcsError {}

/// Central ECS storage containing entity state and component tables.
#[derive(Default)]
pub struct World {
    entities: Vec<EntityRecord>,
    free_list: Vec<u32>,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free_list.pop() {
            let record = &mut self.entities[index as usize];
            record.alive = true;
            Entity::new(index, record.generation)
        } else {
            let index = self.entities.len() as u32;
            self.entities.push(EntityRecord {
                generation: 0,
                alive: true,
            });
            Entity::new(index, 0)
        }
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), EcsError> {
        self.validate_entity(entity)?;
        let record = &mut self.entities[entity.index as usize];
        record.alive = false;
        record.generation = record.generation.wrapping_add(1);
        for storage in self.storages.values_mut() {
            storage.remove(entity);
        }
        if !self.free_list.contains(&entity.index) {
            self.free_list.push(entity.index);
        }
        Ok(())
    }

    pub fn register_component<T: Component>(&mut self) {
        use std::collections::hash_map::Entry;

        match self.storages.entry(TypeId::of::<T>()) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(Box::new(ComponentMap::<T>::default()));
            }
        }
    }

    pub fn insert<T: Component>(
        &mut self,
        entity: Entity,
        component: T,
    ) -> Result<Option<T>, EcsError> {
        self.validate_entity(entity)?;
        let storage = self.ensure_component_storage::<T>();
        Ok(storage.insert(entity, component))
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        if !self.contains(entity) {
            return None;
        }
        self.typed_storage::<T>()?.get(entity)
    }

    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        if !self.contains(entity) {
            return None;
        }
        self.typed_storage_mut::<T>()?.get_mut(entity)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities
            .get(entity.index as usize)
            .map(|record| record.alive && record.generation == entity.generation)
            .unwrap_or(false)
    }

    pub fn component_entries<T: Component>(&self) -> Vec<(Entity, &T)> {
        self.typed_storage::<T>()
            .map(|storage| storage.iter().map(|(entity, value)| (*entity, value)).collect())
            .unwrap_or_default()
    }

    fn validate_entity(&self, entity: Entity) -> Result<(), EcsError> {
        if self.contains(entity) {
            Ok(())
        } else {
            Err(EcsError::NoSuchEntity(entity))
        }
    }

    fn typed_storage<T: Component>(&self) -> Option<&ComponentMap<T>> {
        self.storages
            .get(&TypeId::of::<T>())
            .and_then(|storage| storage.as_any().downcast_ref::<ComponentMap<T>>())
    }

    fn typed_storage_mut<T: Component>(&mut self) -> Option<&mut ComponentMap<T>> {
        self.storages
            .get_mut(&TypeId::of::<T>())
            .and_then(|storage| storage.as_any_mut().downcast_mut::<ComponentMap<T>>())
    }

    fn ensure_component_storage<T: Component>(&mut self) -> &mut ComponentMap<T> {
        if !self.storages.contains_key(&TypeId::of::<T>()) {
            self.register_component::<T>();
        }
        self.typed_storage_mut::<T>()
            .expect("component storage just registered")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Position(f32, f32, f32);

    #[derive(Debug, PartialEq)]
    struct Velocity(f32, f32, f32);

    #[derive(Debug, PartialEq)]
    struct Health(u32);

    #[test]
    fn spawn_and_insert_component() {
        let mut world = World::new();
        world.register_component::<Position>();

        let entity = world.spawn();
        world
            .insert(entity, Position(1.0, 2.0, 3.0))
            .expect("entity alive");

        assert_eq!(
            world.get::<Position>(entity),
            Some(&Position(1.0, 2.0, 3.0))
        );
    }

    #[test]
    fn despawn_removes_components() {
        let mut world = World::new();
        world.register_component::<Velocity>();
        let entity = world.spawn();
        world.insert(entity, Velocity(0.0, 1.0, 0.0)).unwrap();

        world.despawn(entity).unwrap();

        assert!(!world.contains(entity));
        assert!(world.get::<Velocity>(entity).is_none());
    }

    #[test]
    fn generations_increment_after_despawn() {
        let mut world = World::new();
        let entity_a = world.spawn();
        world.despawn(entity_a).unwrap();

        let entity_b = world.spawn();
        assert_ne!(entity_a, entity_b);
        assert_eq!(entity_a.index(), entity_b.index());
        assert_ne!(entity_a.generation(), entity_b.generation());
    }

    #[test]
    fn insert_auto_registers_storage() {
        let mut world = World::new();
        let entity = world.spawn();

        world
            .insert(entity, Health(42))
            .expect("entity should be valid");

        assert_eq!(world.get::<Health>(entity), Some(&Health(42)));
    }

    #[test]
    fn despawn_invalid_entity_fails() {
        let mut world = World::new();
        let stale = Entity::new(999, 1);

        let err = world.despawn(stale).expect_err("stale entity should error");
        match err {
            EcsError::NoSuchEntity(entity) => assert_eq!(entity, stale),
        }
    }
}
