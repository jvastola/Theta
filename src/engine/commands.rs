use crate::editor::commands::{CMD_SELECTION_HIGHLIGHT, SelectionHighlightCommand};
use crate::network::command_log::{
    AuthorId, CommandAuthor, CommandDefinition, CommandEntry, CommandId, CommandLog,
    CommandLogError, CommandPacket, CommandPayload, CommandRegistry, CommandRole, CommandScope,
    CommandSigner, ConflictStrategy, NoopCommandSigner, NoopSignatureVerifier, SignatureVerifier,
};
#[cfg(feature = "network-quic")]
use crate::network::transport::TransportSession;
use crate::network::{EntityHandle, NetworkSession};
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub struct CommandPipeline {
    log: CommandLog,
    signer: Box<dyn CommandSigner>,
    session: NetworkSession,
    last_published: Option<CommandId>,
    pending_packets: Vec<CommandPacket>,
    metrics: CommandMetricsInternal,
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
        let signer: Box<dyn CommandSigner> = Box::new(NoopCommandSigner::new(author));

        Self {
            log,
            signer,
            session: NetworkSession::connect(),
            last_published: None,
            pending_packets: Vec::new(),
            metrics: CommandMetricsInternal::default(),
        }
    }

    fn append_payload(
        &mut self,
        payload: CommandPayload,
        strategy: Option<ConflictStrategy>,
    ) -> Result<(), CommandLogError> {
        let strategy_hint = strategy.unwrap_or(ConflictStrategy::LastWriteWins);
        let append_result = self
            .log
            .append_local(self.signer.as_ref(), payload, strategy);

        if let Err(err) = append_result {
            if matches!(
                err,
                CommandLogError::ConflictRejected
                    | CommandLogError::Duplicate
                    | CommandLogError::InsufficientPermissions { .. }
            ) {
                self.metrics.record_conflict(strategy_hint);
            }
            return Err(err);
        }

        self.metrics.record_local_append();

        let new_entries = self.log.entries_since(self.last_published.as_ref());
        if !new_entries.is_empty() {
            self.last_published = self.log.latest_id();
            let batch = self.session.craft_command_batch(new_entries);
            let packet =
                CommandPacket::from_batch(&batch).expect("serialize command batch for transport");
            self.pending_packets.push(packet);
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

    pub fn drain_packets(&mut self) -> Vec<CommandPacket> {
        self.pending_packets.drain(..).collect()
    }

    #[cfg_attr(not(any(feature = "network-quic", test)), allow(dead_code))]
    pub fn integrate_remote_packet(
        &mut self,
        packet: &CommandPacket,
    ) -> Result<Vec<CommandEntry>, CommandLogError> {
        let batch = packet
            .decode()
            .map_err(|err| CommandLogError::PacketDecodeFailed(err.to_string()))?;

        let mut applied = Vec::new();
        for entry in batch.entries {
            let start = Instant::now();
            let result = self.log.integrate_remote(entry.clone());
            let latency_ms = start.elapsed().as_secs_f32() * 1000.0;
            self.metrics.record_signature_latency(latency_ms);

            match result {
                Ok(true) => applied.push(entry),
                Ok(false) => {
                    self.metrics.record_conflict(entry.strategy);
                }
                Err(CommandLogError::ConflictRejected) => {
                    self.metrics.record_conflict(entry.strategy);
                    log::warn!(
                        "[commands] remote command {:?} rejected by conflict strategy",
                        entry.id
                    );
                }
                Err(CommandLogError::Duplicate) => {
                    self.metrics.record_conflict(entry.strategy);
                    log::debug!("[commands] duplicate remote command {:?} ignored", entry.id);
                }
                Err(CommandLogError::InsufficientPermissions { .. }) => {
                    self.metrics.record_conflict(entry.strategy);
                    log::warn!(
                        "[commands] remote command {:?} failed permission check",
                        entry.id
                    );
                }
                Err(err) => return Err(err),
            }
        }

        if let Some(latest) = self.log.latest_id() {
            self.last_published = Some(latest);
        }

        Ok(applied)
    }

    pub fn update_queue_depth(&mut self, depth: usize) {
        self.metrics.update_queue_depth(depth);
    }

    pub fn metrics_snapshot(&self) -> CommandMetricsSnapshot {
        self.metrics.snapshot()
    }

    #[allow(dead_code)]
    pub fn set_signer(&mut self, signer: Box<dyn CommandSigner>) {
        self.signer = signer;
    }

    #[allow(dead_code)]
    pub fn set_signature_verifier(&mut self, verifier: Arc<dyn SignatureVerifier>) {
        self.log.set_verifier(verifier);
    }

    #[allow(dead_code)]
    pub fn replace_network_session(&mut self, session: NetworkSession) {
        self.session = session;
    }

    #[cfg(feature = "network-quic")]
    pub fn attach_transport_session(&mut self, session: &TransportSession) {
        self.session.attach_transport_session(session);
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CommandMetricsSnapshot {
    pub total_appended: u64,
    pub append_rate_per_sec: f32,
    pub conflict_rejections: std::collections::HashMap<ConflictStrategy, u64>,
    pub queue_depth: usize,
    pub signature_verify_latency_ms: f32,
}

#[derive(Default)]
struct CommandMetricsInternal {
    total_appended: u64,
    append_rate_per_sec: f32,
    last_append_time: Option<Instant>,
    conflict_rejections: HashMap<ConflictStrategy, u64>,
    signature_verify_latency_ms: f32,
    queue_depth: usize,
}

impl CommandMetricsInternal {
    fn record_local_append(&mut self) {
        self.total_appended = self.total_appended.saturating_add(1);
        let now = Instant::now();
        if let Some(last) = self.last_append_time {
            let delta = now.duration_since(last).as_secs_f32();
            if delta > 0.000_1 {
                let instantaneous = 1.0 / delta;
                self.append_rate_per_sec = if self.append_rate_per_sec == 0.0 {
                    instantaneous
                } else {
                    let alpha = 0.2;
                    self.append_rate_per_sec * (1.0 - alpha) + instantaneous * alpha
                };
            }
        }
        self.last_append_time = Some(now);
    }

    fn record_conflict(&mut self, strategy: ConflictStrategy) {
        let entry = self.conflict_rejections.entry(strategy).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    fn record_signature_latency(&mut self, latency_ms: f32) {
        if latency_ms <= 0.0 {
            return;
        }
        if self.signature_verify_latency_ms == 0.0 {
            self.signature_verify_latency_ms = latency_ms;
        } else {
            let alpha = 0.2;
            self.signature_verify_latency_ms =
                self.signature_verify_latency_ms * (1.0 - alpha) + latency_ms * alpha;
        }
    }

    fn update_queue_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
    }

    fn snapshot(&self) -> CommandMetricsSnapshot {
        CommandMetricsSnapshot {
            total_appended: self.total_appended,
            append_rate_per_sec: self.append_rate_per_sec,
            conflict_rejections: self.conflict_rejections.clone(),
            queue_depth: self.queue_depth,
            signature_verify_latency_ms: self.signature_verify_latency_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::command_log::CommandBatch;

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

        let packets = pipeline.drain_packets();
        assert_eq!(packets.len(), 1);
        let batch = packets[0].decode().expect("decode command packet payload");
        assert_eq!(batch.entries.len(), 1);
        let entry = &batch.entries[0];
        assert_eq!(entry.payload.command_type, CMD_SELECTION_HIGHLIGHT);
        assert_eq!(entry.payload.scope, CommandScope::Entity(entity));

        let decoded: SelectionHighlightCommand =
            serde_json::from_slice(&entry.payload.data).expect("decode highlight command");
        assert_eq!(decoded.entity, entity);
        assert!(decoded.active);

        // no extra batches when nothing new happens
        let none = pipeline.drain_packets();
        assert!(none.is_empty());
    }

    #[test]
    fn integrates_remote_packet_and_updates_lamport() {
        let mut pipeline = CommandPipeline::new();
        let entity = EntityHandle {
            index: 9,
            generation: 0,
        };

        pipeline
            .record_selection_highlight(entity, true)
            .expect("append highlight");
        let local_packets = pipeline.drain_packets();
        assert_eq!(local_packets.len(), 1);

        let remote = SelectionHighlightCommand::new(entity, false);
        let payload = serde_json::to_vec(&remote).expect("serialize remote command");
        let author = CommandAuthor::new(AuthorId(11), CommandRole::Editor);
        let entry = CommandEntry::new(
            CommandId::new(5, AuthorId(11)),
            777,
            CommandPayload::new(
                CMD_SELECTION_HIGHLIGHT,
                CommandScope::Entity(entity),
                payload,
            ),
            ConflictStrategy::LastWriteWins,
            author,
            None,
        );

        let batch = CommandBatch {
            sequence: 2,
            timestamp_ms: 888,
            entries: vec![entry.clone()],
        };
        let packet = CommandPacket::from_batch(&batch).expect("packet serialize");

        let applied = pipeline
            .integrate_remote_packet(&packet)
            .expect("integrate remote");
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].id, entry.id);

        assert_eq!(pipeline.last_published, pipeline.log.latest_id());

        pipeline
            .record_selection_highlight(entity, true)
            .expect("append second");
        let packets = pipeline.drain_packets();
        assert_eq!(packets.len(), 1);
        let decoded = packets[0].decode().expect("decode batch");
        assert_eq!(decoded.entries.len(), 1);
        assert!(decoded.entries[0].id.lamport() > entry.id.lamport());
    }
}
