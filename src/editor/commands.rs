use crate::network::EntityHandle;
use crate::network::command_log::CommandBatch;
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
}
