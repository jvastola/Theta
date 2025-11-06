use crate::network::EntityHandle;
use crate::network::command_log::{CommandBatch, CommandPacket};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CMD_ENTITY_TRANSLATE: &str = "editor.entity.translate";
pub const CMD_ENTITY_ROTATE: &str = "editor.entity.rotate";
pub const CMD_ENTITY_SCALE: &str = "editor.entity.scale";
pub const CMD_TOOL_ACTIVATE: &str = "editor.tool.activate";
pub const CMD_TOOL_DEACTIVATE: &str = "editor.tool.deactivate";
pub const CMD_MESH_VERTEX_CREATE: &str = "editor.mesh.vertex_create";
pub const CMD_MESH_EDGE_EXTRUDE: &str = "editor.mesh.edge_extrude";
pub const CMD_MESH_FACE_SUBDIVIDE: &str = "editor.mesh.face_subdivide";

pub const CMD_SELECTION_HIGHLIGHT: &str = "editor.selection.highlight";

fn deserialize_metadata<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let option = Option::<HashMap<String, String>>::deserialize(deserializer)?;
    Ok(option.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionHighlightCommand {
    pub entity: EntityHandle,
    pub active: bool,
}

impl SelectionHighlightCommand {
    pub fn new(entity: EntityHandle, active: bool) -> Self {
        Self { entity, active }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityTranslateCommand {
    pub entity: EntityHandle,
    pub delta: [f32; 3],
}

impl EntityTranslateCommand {
    pub fn new(entity: EntityHandle, delta: [f32; 3]) -> Self {
        Self { entity, delta }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityRotateCommand {
    pub entity: EntityHandle,
    pub rotation: Quaternion,
}

impl EntityRotateCommand {
    pub fn new(entity: EntityHandle, rotation: Quaternion) -> Self {
        Self { entity, rotation }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityScaleCommand {
    pub entity: EntityHandle,
    pub scale: [f32; 3],
}

impl EntityScaleCommand {
    pub fn new(entity: EntityHandle, scale: [f32; 3]) -> Self {
        Self { entity, scale }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolActivateCommand {
    pub tool_id: String,
}

impl ToolActivateCommand {
    pub fn new(tool_id: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolDeactivateCommand {
    pub tool_id: String,
}

impl ToolDeactivateCommand {
    pub fn new(tool_id: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VertexCreateCommand {
    pub position: [f32; 3],
    #[serde(default, deserialize_with = "deserialize_metadata")]
    pub metadata: HashMap<String, String>,
}

impl VertexCreateCommand {
    pub fn new(position: [f32; 3], metadata: HashMap<String, String>) -> Self {
        Self { position, metadata }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdgeExtrudeCommand {
    pub edge_id: u32,
    pub direction: [f32; 3],
}

impl EdgeExtrudeCommand {
    pub fn new(edge_id: u32, direction: [f32; 3]) -> Self {
        Self { edge_id, direction }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubdivideParams {
    pub levels: u32,
    pub smoothness: f32,
}

impl Default for SubdivideParams {
    fn default() -> Self {
        Self {
            levels: 1,
            smoothness: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FaceSubdivideCommand {
    pub face_id: u32,
    #[serde(default)]
    pub params: SubdivideParams,
}

impl FaceSubdivideCommand {
    pub fn new(face_id: u32, params: SubdivideParams) -> Self {
        Self { face_id, params }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternion {
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

impl Default for Quaternion {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CommandOutbox {
    pending: Vec<CommandBatch>,
    history: Vec<CommandBatch>,
    transmissions: Vec<CommandPacket>,
}

impl CommandOutbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest<I>(&mut self, batches: I)
    where
        I: IntoIterator<Item = CommandBatch>,
    {
        for batch in batches {
            self.pending.push(batch.clone());
            self.history.push(batch);
        }
    }

    pub fn drain_pending(&mut self) -> Vec<CommandBatch> {
        self.pending.drain(..).collect()
    }

    pub fn last_published(&self) -> Option<&CommandBatch> {
        self.history.last()
    }

    pub fn total_batches(&self) -> usize {
        self.history.len()
    }

    pub fn total_entries(&self) -> usize {
        self.history.iter().map(|batch| batch.entries.len()).sum()
    }

    pub fn total_packets(&self) -> usize {
        self.transmissions.len()
    }

    pub fn last_packet(&self) -> Option<&CommandPacket> {
        self.transmissions.last()
    }

    pub fn drain_packets(&mut self) -> Vec<CommandPacket> {
        let pending = self.drain_pending();
        let mut packets = Vec::with_capacity(pending.len());
        for batch in pending {
            match CommandPacket::from_batch(&batch) {
                Ok(packet) => {
                    self.transmissions.push(packet.clone());
                    packets.push(packet);
                }
                Err(err) => {
                    log::error!(
                        "[commands] failed to serialize command batch {}: {err}",
                        batch.sequence
                    );
                }
            }
        }
        packets
    }
}

#[derive(Debug, Default, Clone)]
pub struct CommandTransportQueue {
    pending: Vec<CommandPacket>,
    sent: Vec<CommandPacket>,
}

impl CommandTransportQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue<I>(&mut self, packets: I)
    where
        I: IntoIterator<Item = CommandPacket>,
    {
        for packet in packets {
            self.pending.push(packet.clone());
            self.sent.push(packet);
        }
    }

    pub fn drain_pending(&mut self) -> Vec<CommandPacket> {
        self.pending.drain(..).collect()
    }

    pub fn total_transmissions(&self) -> usize {
        self.sent.len()
    }

    pub fn last_packet(&self) -> Option<&CommandPacket> {
        self.sent.last()
    }

    pub fn pending_depth(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::command_log::{
        AuthorId, CommandAuthor, CommandBatch, CommandEntry, CommandId, CommandPayload,
        CommandRole, CommandScope, ConflictStrategy,
    };
    use std::collections::HashMap;

    #[test]
    fn outbox_accumulates_and_drains_batches() {
        let mut outbox = CommandOutbox::new();
        let entity = EntityHandle {
            index: 1,
            generation: 0,
        };
        let payload = CommandPayload::new(
            CMD_SELECTION_HIGHLIGHT,
            CommandScope::Entity(entity),
            vec![1, 2, 3],
        );
        let author = CommandAuthor::new(AuthorId(7), CommandRole::Editor);
        let entry = CommandEntry::new(
            CommandId::new(1, AuthorId(7)),
            1234,
            payload,
            ConflictStrategy::LastWriteWins,
            author,
            None,
        );
        let batch = CommandBatch {
            sequence: 5,
            nonce: 1,
            timestamp_ms: 999,
            author: AuthorId(7),
            entries: vec![entry],
        };

        outbox.ingest(vec![batch]);
        assert_eq!(outbox.total_batches(), 1);
        assert_eq!(outbox.total_entries(), 1);
        assert!(outbox.last_published().is_some());

        let pending = outbox.drain_pending();
        assert_eq!(pending.len(), 1);
        assert!(outbox.drain_pending().is_empty());
    }

    #[test]
    fn outbox_serializes_packets_and_tracks_transmissions() {
        let mut outbox = CommandOutbox::new();
        let entity = EntityHandle {
            index: 2,
            generation: 0,
        };
        let payload = CommandPayload::new(
            CMD_SELECTION_HIGHLIGHT,
            CommandScope::Entity(entity),
            vec![9, 9, 9],
        );
        let entry = CommandEntry::new(
            CommandId::new(7, AuthorId(1)),
            42,
            payload,
            ConflictStrategy::LastWriteWins,
            CommandAuthor::new(AuthorId(1), CommandRole::Editor),
            None,
        );
        let batch = CommandBatch {
            sequence: 3,
            nonce: 7,
            timestamp_ms: 555,
            author: AuthorId(1),
            entries: vec![entry],
        };

        outbox.ingest(vec![batch]);
        let packets = outbox.drain_packets();
        assert_eq!(packets.len(), 1);
        assert_eq!(outbox.total_packets(), 1);
        let packet = packets.first().expect("packet present");
        assert_eq!(packet.sequence, 3);
        assert!(!packet.payload.is_empty());

        // subsequent drains should be empty but history retained
        assert!(outbox.drain_packets().is_empty());
        assert_eq!(outbox.total_batches(), 1);
    }

    #[test]
    fn transport_queue_tracks_transmissions() {
        let mut queue = CommandTransportQueue::new();
        let packet = CommandPacket {
            sequence: 1,
            nonce: 1,
            timestamp_ms: 100,
            payload: vec![1, 2, 3],
        };

        queue.enqueue(vec![packet.clone()]);
        assert_eq!(queue.total_transmissions(), 1);
        assert!(queue.last_packet().is_some());

        let drained = queue.drain_pending();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].sequence, 1);
        assert!(queue.drain_pending().is_empty());
    }

    #[test]
    fn mesh_commands_serialize_correctly() {
        let mut metadata = HashMap::new();
        metadata.insert("material".into(), "clay".into());
        metadata.insert("symmetry".into(), "x".into());

        let vertex = VertexCreateCommand::new([1.0, 2.0, 3.5], metadata.clone());
        let encoded = serde_json::to_string(&vertex).expect("serialize vertex command");
        let restored: VertexCreateCommand =
            serde_json::from_str(&encoded).expect("deserialize vertex command");
        assert_eq!(restored.position, vertex.position);
        assert_eq!(restored.metadata, vertex.metadata);

        let extrude = EdgeExtrudeCommand::new(77, [0.0, 1.0, 0.0]);
        let encoded = serde_json::to_string(&extrude).expect("serialize edge extrude");
        let restored: EdgeExtrudeCommand =
            serde_json::from_str(&encoded).expect("deserialize edge extrude");
        assert_eq!(restored.edge_id, extrude.edge_id);
        assert_eq!(restored.direction, extrude.direction);

        let subdivide = FaceSubdivideCommand::new(
            91,
            SubdivideParams {
                levels: 3,
                smoothness: 0.25,
            },
        );
        let encoded = serde_json::to_string(&subdivide).expect("serialize subdivide");
        let restored: FaceSubdivideCommand =
            serde_json::from_str(&encoded).expect("deserialize subdivide");
        assert_eq!(restored.face_id, subdivide.face_id);
        assert_eq!(restored.params.levels, subdivide.params.levels);
        assert!((restored.params.smoothness - subdivide.params.smoothness).abs() < f32::EPSILON);

        let decoded: VertexCreateCommand =
            serde_json::from_str("{\"position\":[0,0,0]}").expect("metadata should default");
        assert!(decoded.metadata.is_empty());
    }
}
