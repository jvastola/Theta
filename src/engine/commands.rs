use crate::editor::commands::{CMD_SELECTION_HIGHLIGHT, SelectionHighlightCommand};
use crate::network::command_log::{
    AuthorId, CommandAuthor, CommandBatch, CommandDefinition, CommandId, CommandLog,
    CommandLogError, CommandPayload, CommandRegistry, CommandRole, CommandScope, ConflictStrategy,
    NoopCommandSigner, NoopSignatureVerifier, SignatureVerifier,
};
use crate::network::{EntityHandle, NetworkSession};
use serde_json::to_vec;
use std::sync::Arc;

pub struct CommandPipeline {
    log: CommandLog,
    signer: NoopCommandSigner,
    session: NetworkSession,
    last_published: Option<CommandId>,
    pending_batches: Vec<CommandBatch>,
}

impl CommandPipeline {
    pub fn new() -> Self {
        let mut registry = CommandRegistry::new();
        registry.register(
            CMD_SELECTION_HIGHLIGHT,
            CommandDefinition::builder()
                .required_role(CommandRole::Editor)
                .default_strategy(ConflictStrategy::LastWriteWins)
                .require_signature(false)
                .build(),
        );
        let registry = Arc::new(registry);
        let verifier = Arc::new(NoopSignatureVerifier::default()) as Arc<dyn SignatureVerifier>;
        let log = CommandLog::new(Arc::clone(&registry), verifier);
        let author = CommandAuthor::new(AuthorId(0), CommandRole::Editor);
        let signer = NoopCommandSigner::new(author);

        Self {
            log,
            signer,
            session: NetworkSession::connect(),
            last_published: None,
            pending_batches: Vec::new(),
        }
    }

    fn append_payload(
        &mut self,
        payload: CommandPayload,
        strategy: Option<ConflictStrategy>,
    ) -> Result<(), CommandLogError> {
        self.log.append_local(&self.signer, payload, strategy)?;
        let new_entries = self.log.entries_since(self.last_published.as_ref());
        if !new_entries.is_empty() {
            self.last_published = self.log.latest_id();
            let batch = self.session.craft_command_batch(new_entries);
            self.pending_batches.push(batch);
        }
        Ok(())
    }

    pub fn record_selection_highlight(
        &mut self,
        entity: EntityHandle,
        active: bool,
    ) -> Result<(), CommandLogError> {
        let command = SelectionHighlightCommand::new(entity, active);
        let data = to_vec(&command).expect("serialize highlight command");
        let payload =
            CommandPayload::new(CMD_SELECTION_HIGHLIGHT, CommandScope::Entity(entity), data);
        self.append_payload(payload, Some(ConflictStrategy::LastWriteWins))
    }

    pub fn drain_batches(&mut self) -> Vec<CommandBatch> {
        self.pending_batches.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_emits_batches_for_highlight() {
        let mut pipeline = CommandPipeline::new();
        let entity = EntityHandle {
            index: 5,
            generation: 1,
        };

        pipeline
            .record_selection_highlight(entity, true)
            .expect("append highlight");

        let batches = pipeline.drain_batches();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(batch.entries.len(), 1);
        let entry = &batch.entries[0];
        assert_eq!(entry.payload.command_type, CMD_SELECTION_HIGHLIGHT);
        assert_eq!(entry.payload.scope, CommandScope::Entity(entity));

        let decoded: SelectionHighlightCommand =
            serde_json::from_slice(&entry.payload.data).expect("decode highlight command");
        assert_eq!(decoded.entity, entity);
        assert!(decoded.active);

        // no extra batches when nothing new happens
        let none = pipeline.drain_batches();
        assert!(none.is_empty());
    }
}
