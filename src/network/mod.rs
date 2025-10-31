pub mod command_log;
pub mod replication;
pub mod schema;

#[cfg(feature = "network-quic")]
pub mod transport;

#[cfg(not(feature = "network-quic"))]
pub mod transport {
    use super::TransportDiagnostics;

    #[derive(Clone, Default)]
    pub struct TransportMetricsHandle;

    impl TransportMetricsHandle {
        pub fn new() -> Self {
            Self
        }

        pub fn latest(&self) -> Option<TransportDiagnostics> {
            None
        }
    }
}

#[cfg(has_generated_network_schema)]
#[allow(dead_code)]
pub mod wire {
    include!(concat!(
        env!("OUT_DIR"),
        "/flatbuffers/network_generated.rs"
    ));
}

#[cfg(not(has_generated_network_schema))]
#[allow(dead_code)]
pub mod wire {
    //! Stub module used when FlatBuffers bindings have not been generated yet.
    //! Build scripts set the `has_generated_network_schema` cfg once `flatc`
    //! runs successfully.
}

use crate::ecs::Entity;
use crate::network::command_log::{CommandBatch, CommandEntry};
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TransportDiagnostics {
    pub rtt_ms: f32,
    pub jitter_ms: f32,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub compression_ratio: f32,
    pub command_packets_sent: u64,
    pub command_packets_received: u64,
    pub command_bandwidth_bytes_per_sec: f32,
    pub command_latency_ms: f32,
}

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
    transport_metrics: Option<transport::TransportMetricsHandle>,
}

impl NetworkSession {
    pub fn new() -> Self {
        Self {
            sequence: 0,
            advertised_components: Vec::new(),
            transport_metrics: None,
        }
    }

    pub fn connect() -> Self {
        Self::new()
    }

    pub fn with_transport_metrics(handle: transport::TransportMetricsHandle) -> Self {
        Self {
            sequence: 0,
            advertised_components: Vec::new(),
            transport_metrics: Some(handle),
        }
    }

    pub fn attach_transport_metrics(&mut self, handle: transport::TransportMetricsHandle) {
        self.transport_metrics = Some(handle);
    }

    pub fn transport_metrics(&self) -> Option<TransportDiagnostics> {
        self.transport_metrics
            .as_ref()
            .and_then(|handle| handle.latest())
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_transport_session(&mut self, session: &transport::TransportSession) {
        self.transport_metrics = Some(session.metrics_handle());
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

    pub fn craft_command_batch(&mut self, entries: Vec<CommandEntry>) -> CommandBatch {
        CommandBatch {
            sequence: self.next_sequence(),
            timestamp_ms: current_time_millis(),
            entries,
        }
    }
}

pub(crate) fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::command_log::{
        AuthorId, CommandAuthor, CommandEntry, CommandId, CommandPayload, CommandRole,
        CommandScope, ConflictStrategy,
    };

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
        use super::wire::theta::net::{
            self, Compression, MessageBody, MessageEnvelopeArgs, PacketHeaderArgs, SessionHelloArgs,
        };
        use flatbuffers::{FlatBufferBuilder, UnionWIPOffset};

        let mut builder = FlatBufferBuilder::new();
        let client_nonce = builder.create_vector(&[1u8, 2, 3, 4]);
        let capabilities = builder.create_vector(&[7u32, 11u32]);
        let auth_token = builder.create_string("token");
        let client_public_key = builder.create_vector(&[9u8; 32]);

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
                client_public_key: Some(client_public_key),
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

        let session = parsed.body_as_session_hello().expect("session hello body");
        assert_eq!(session.protocol_version(), 1);
        assert_eq!(session.schema_hash(), 0xDEAD_BEEFu64);
        let nonce: Vec<u8> = session.client_nonce().expect("nonce").iter().collect();
        assert_eq!(nonce, vec![1, 2, 3, 4]);
        let caps: Vec<u32> = session
            .requested_capabilities()
            .expect("capabilities")
            .iter()
            .collect();
        assert_eq!(caps, vec![7, 11]);
        assert_eq!(session.auth_token(), Some("token"));
        let public_key: Vec<u8> = session
            .client_public_key()
            .expect("client public key")
            .iter()
            .collect();
        assert_eq!(public_key.len(), 32);
        assert!(public_key.iter().all(|byte| *byte == 9));
    }

    #[test]
    fn advertised_components_deduplicate_between_change_sets() {
        let mut session = NetworkSession::new();
        let descriptor = ComponentDescriptor {
            key: ComponentKey::of::<Position>(),
        };

        session.advertise_component(descriptor.clone());
        session.advertise_component(descriptor.clone());

        let first = session.craft_change_set(Vec::new());
        assert_eq!(first.descriptors.len(), 1);

        let second = session.craft_change_set(Vec::new());
        assert!(second.descriptors.is_empty());
    }

    #[test]
    fn change_set_sequence_monotonic() {
        let mut session = NetworkSession::new();
        let entity = Entity::new(7, 2);
        let diff = ComponentDiff::update::<Position>(entity, vec![42]);

        let first = session.craft_change_set(vec![diff.clone()]);
        let second = session.craft_change_set(vec![diff]);

        assert_eq!(first.sequence + 1, second.sequence);
        assert!(first.timestamp_ms <= second.timestamp_ms);
    }

    #[test]
    fn command_batch_sequences_increment() {
        let mut session = NetworkSession::new();
        let author = CommandAuthor::new(AuthorId(5), CommandRole::Editor);
        let entry = CommandEntry::new(
            CommandId::new(1, AuthorId(5)),
            99,
            CommandPayload::new("editor.test", CommandScope::Global, vec![9]),
            ConflictStrategy::Merge,
            author.clone(),
            None,
        );

        let first = session.craft_command_batch(vec![entry.clone()]);
        assert_eq!(first.sequence, 1);
        assert_eq!(first.entries.len(), 1);
        assert!(first.timestamp_ms <= current_time_millis());

        let second = session.craft_command_batch(vec![entry]);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.entries.len(), 1);
    }
}
