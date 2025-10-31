use crate::network::EntityHandle;
use crate::network::command_log::{CommandBatch, CommandPacket};
use serde::{Deserialize, Serialize};

pub const CMD_SELECTION_HIGHLIGHT: &str = "editor.selection.highlight";

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
            timestamp_ms: 999,
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
            timestamp_ms: 555,
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
}
