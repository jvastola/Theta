pub mod schema;

#[cfg(has_generated_network_schema)]
#[allow(dead_code)]
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/flatbuffers/network_generated.rs"));
}

#[cfg(not(has_generated_network_schema))]
#[allow(dead_code)]
pub mod wire {
    //! Stub module used when FlatBuffers bindings have not been generated yet.
    //! Build scripts set the `has_generated_network_schema` cfg once `flatc`
    //! runs successfully.
}

use crate::ecs::Entity;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
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
            type_hash: crate::network::schema::stable_component_hash(type_name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ComponentDescriptor {
    pub key: ComponentKey,
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
    pub descriptors: Vec<ComponentDescriptor>,
}

impl ChangeSet {
    pub fn new(
        sequence: u64,
        timestamp_ms: u64,
        diffs: Vec<ComponentDiff>,
        descriptors: Vec<ComponentDescriptor>,
    ) -> Self {
        Self {
            sequence,
            timestamp_ms,
            diffs,
            descriptors,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }
}

pub struct NetworkSession {
    sequence: u64,
    advertised_components: Vec<ComponentDescriptor>,
}

impl NetworkSession {
    pub fn new() -> Self {
        Self {
            sequence: 0,
            advertised_components: Vec::new(),
        }
    }

    pub fn connect() -> Self {
        Self::new()
    }

    pub fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    pub fn advertise_component(&mut self, descriptor: ComponentDescriptor) {
        if !self.advertised_components.contains(&descriptor) {
            self.advertised_components.push(descriptor);
        }
    }

    pub fn craft_change_set(&mut self, diffs: Vec<ComponentDiff>) -> ChangeSet {
        let sequence = self.next_sequence();
        let timestamp_ms = current_time_millis();
        let descriptors = std::mem::take(&mut self.advertised_components);
        ChangeSet::new(sequence, timestamp_ms, diffs, descriptors)
    }
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
        session.advertise_component(ComponentDescriptor {
            key: ComponentKey::of::<Position>(),
        });
        let change_set = session.craft_change_set(vec![diff]);

        let output = serde_json::to_string(&change_set).expect("serialization should succeed");
        assert!(output.contains("sequence"));
        assert!(output.contains("descriptors"));
    }

    #[cfg(has_generated_network_schema)]
    #[test]
    fn flatbuffer_session_hello_roundtrip() {
        use flatbuffers::{FlatBufferBuilder, UnionWIPOffset};
        use super::wire::theta::net::{self, Compression, MessageBody, MessageEnvelopeArgs, PacketHeaderArgs, SessionHelloArgs};

        let mut builder = FlatBufferBuilder::new();
        let client_nonce = builder.create_vector(&[1u8, 2, 3, 4]);
        let capabilities = builder.create_vector(&[7u32, 11u32]);
        let auth_token = builder.create_string("token");

        let header = net::PacketHeader::create(
            &mut builder,
            &PacketHeaderArgs {
                sequence_id: 42,
                timestamp_ms: 1234,
                compression: Compression::None,
                schema_hash: 0xDEAD_BEEFu64,
            },
        );

        let hello = net::SessionHello::create(
            &mut builder,
            &SessionHelloArgs {
                protocol_version: 1,
                schema_hash: 0xDEAD_BEEFu64,
                client_nonce: Some(client_nonce),
                requested_capabilities: Some(capabilities),
                auth_token: Some(auth_token),
            },
        );

    let hello_union = flatbuffers::WIPOffset::<UnionWIPOffset>::new(hello.value());

        let envelope = net::MessageEnvelope::create(
            &mut builder,
            &MessageEnvelopeArgs {
                header: Some(header),
                body_type: MessageBody::SessionHello,
                body: Some(hello_union),
            },
        );

        net::finish_message_envelope_buffer(&mut builder, envelope);
        let bytes = builder.finished_data();

        let parsed = net::root_as_message_envelope(bytes).expect("valid envelope");
        let parsed_header = parsed.header().expect("header");
        assert_eq!(parsed_header.sequence_id(), 42);
        assert_eq!(parsed_header.timestamp_ms(), 1234);
        assert_eq!(parsed_header.compression(), Compression::None);
        assert_eq!(parsed_header.schema_hash(), 0xDEAD_BEEFu64);

        let session = parsed
            .body_as_session_hello()
            .expect("session hello body");
        assert_eq!(session.protocol_version(), 1);
        assert_eq!(session.schema_hash(), 0xDEAD_BEEFu64);
        let nonce: Vec<u8> = session
            .client_nonce()
            .expect("nonce")
            .iter()
            .collect();
        assert_eq!(nonce, vec![1, 2, 3, 4]);
        let caps: Vec<u32> = session
            .requested_capabilities()
            .expect("capabilities")
            .iter()
            .collect();
        assert_eq!(caps, vec![7, 11]);
        assert_eq!(session.auth_token(), Some("token"));
    }
}
