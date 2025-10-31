use crate::network::EntityHandle;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CommandRole {
    Viewer,
    Editor,
    Admin,
}

impl CommandRole {
    fn allows(self, required: CommandRole) -> bool {
        matches!(
            (self, required),
            (CommandRole::Admin, _)
                | (
                    CommandRole::Editor,
                    CommandRole::Editor | CommandRole::Viewer
                )
                | (CommandRole::Viewer, CommandRole::Viewer)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommandScope {
    Global,
    Entity(EntityHandle),
    Tool(String),
}

impl CommandScope {
    fn key(&self) -> CommandScopeKey {
        match self {
            CommandScope::Global => CommandScopeKey::Global,
            CommandScope::Entity(handle) => CommandScopeKey::Entity(*handle),
            CommandScope::Tool(name) => CommandScopeKey::Tool(name.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CommandScopeKey {
    Global,
    Entity(EntityHandle),
    Tool(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStrategy {
    LastWriteWins,
    Merge,
    Reject,
}

impl Default for ConflictStrategy {
    fn default() -> Self {
        ConflictStrategy::LastWriteWins
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPayload {
    pub command_type: String,
    pub scope: CommandScope,
    pub data: Vec<u8>,
}

impl CommandPayload {
    pub fn new(command_type: impl Into<String>, scope: CommandScope, data: Vec<u8>) -> Self {
        Self {
            command_type: command_type.into(),
            scope,
            data,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandDefinition {
    required_role: CommandRole,
    default_strategy: ConflictStrategy,
    require_signature: bool,
}

impl CommandDefinition {
    pub fn builder() -> CommandDefinitionBuilder {
        CommandDefinitionBuilder::new()
    }

    pub fn required_role(&self) -> CommandRole {
        self.required_role
    }

    pub fn default_strategy(&self) -> ConflictStrategy {
        self.default_strategy
    }

    pub fn require_signature(&self) -> bool {
        self.require_signature
    }
}

pub struct CommandDefinitionBuilder {
    required_role: CommandRole,
    default_strategy: ConflictStrategy,
    require_signature: bool,
}

impl CommandDefinitionBuilder {
    fn new() -> Self {
        Self {
            required_role: CommandRole::Editor,
            default_strategy: ConflictStrategy::LastWriteWins,
            require_signature: true,
        }
    }

    pub fn required_role(mut self, role: CommandRole) -> Self {
        self.required_role = role;
        self
    }

    pub fn default_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.default_strategy = strategy;
        self
    }

    pub fn require_signature(mut self, require: bool) -> Self {
        self.require_signature = require;
        self
    }

    pub fn build(self) -> CommandDefinition {
        CommandDefinition {
            required_role: self.required_role,
            default_strategy: self.default_strategy,
            require_signature: self.require_signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct AuthorId(pub u64);

#[derive(Debug, Clone)]
pub struct AuthorPublicKey(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct CommandAuthor {
    pub id: AuthorId,
    pub role: CommandRole,
    pub public_key: Option<AuthorPublicKey>,
}

impl CommandAuthor {
    pub fn new(id: AuthorId, role: CommandRole) -> Self {
        Self {
            id,
            role,
            public_key: None,
        }
    }

    pub fn with_public_key(mut self, key: AuthorPublicKey) -> Self {
        self.public_key = Some(key);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct CommandId {
    lamport: u64,
    author: AuthorId,
}

impl CommandId {
    pub fn new(lamport: u64, author: AuthorId) -> Self {
        Self { lamport, author }
    }

    pub fn lamport(&self) -> u64 {
        self.lamport
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSignature(pub Vec<u8>);

pub trait SignatureVerifier: Send + Sync {
    fn verify(
        &self,
        author: &CommandAuthor,
        lamport: u64,
        payload: &CommandPayload,
        signature: &CommandSignature,
    ) -> bool;
}

pub trait CommandSigner {
    fn author(&self) -> &CommandAuthor;
    fn sign(&self, lamport: u64, payload: &CommandPayload) -> Option<CommandSignature>;
}

#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub id: CommandId,
    pub timestamp_ms: u64,
    pub payload: CommandPayload,
    pub strategy: ConflictStrategy,
    pub author: CommandAuthor,
    pub signature: Option<CommandSignature>,
}

impl CommandEntry {
    pub fn new(
        id: CommandId,
        timestamp_ms: u64,
        payload: CommandPayload,
        strategy: ConflictStrategy,
        author: CommandAuthor,
        signature: Option<CommandSignature>,
    ) -> Self {
        Self {
            id,
            timestamp_ms,
            payload,
            strategy,
            author,
            signature,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandLogError {
    #[error("unregistered command type: {0}")]
    UnregisteredCommand(String),
    #[error("insufficient permissions: required {required:?}, actual {actual:?}")]
    InsufficientPermissions {
        required: CommandRole,
        actual: CommandRole,
    },
    #[error("signature missing for command type {0}")]
    SignatureMissing(String),
    #[error("signature rejected for author {0:?}")]
    InvalidSignature(AuthorId),
    #[error("command rejected by conflict strategy")]
    ConflictRejected,
    #[error("duplicate command id")]
    Duplicate,
}

#[derive(Default, Clone)]
pub struct CommandRegistry {
    definitions: HashMap<String, CommandDefinition>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    pub fn register(&mut self, command_type: impl Into<String>, def: CommandDefinition) {
        self.definitions.insert(command_type.into(), def);
    }

    fn definition(&self, command_type: &str) -> Option<&CommandDefinition> {
        self.definitions.get(command_type)
    }
}

pub struct CommandLog {
    lamport_clock: u64,
    entries: BTreeMap<CommandId, CommandEntry>,
    latest_by_scope: HashMap<CommandScopeKey, CommandId>,
    registry: Arc<CommandRegistry>,
    verifier: Arc<dyn SignatureVerifier>,
}

impl CommandLog {
    pub fn new(registry: Arc<CommandRegistry>, verifier: Arc<dyn SignatureVerifier>) -> Self {
        Self {
            lamport_clock: 0,
            entries: BTreeMap::new(),
            latest_by_scope: HashMap::new(),
            registry,
            verifier,
        }
    }

    pub fn lamport(&self) -> u64 {
        self.lamport_clock
    }

    fn next_lamport(&mut self) -> u64 {
        self.lamport_clock = self.lamport_clock.wrapping_add(1);
        self.lamport_clock
    }

    pub fn append_local<S: CommandSigner>(
        &mut self,
        signer: &S,
        mut payload: CommandPayload,
        strategy: Option<ConflictStrategy>,
    ) -> Result<CommandId, CommandLogError> {
        let definition = self
            .registry
            .definition(&payload.command_type)
            .ok_or_else(|| CommandLogError::UnregisteredCommand(payload.command_type.clone()))?;
        let required_role = definition.required_role();
        let default_strategy = definition.default_strategy();
        let require_signature = definition.require_signature();

        let author = signer.author();
        if !author.role.allows(required_role) {
            return Err(CommandLogError::InsufficientPermissions {
                required: required_role,
                actual: author.role,
            });
        }

        if matches!(payload.scope, CommandScope::Tool(ref name) if name.trim().is_empty()) {
            payload.scope = CommandScope::Global;
        }

        let lamport = self.next_lamport();
        let id = CommandId::new(lamport, author.id.clone());
        let signature = signer.sign(lamport, &payload);

        if require_signature && signature.is_none() {
            return Err(CommandLogError::SignatureMissing(
                payload.command_type.clone(),
            ));
        }

        let entry = CommandEntry::new(
            id.clone(),
            current_time_millis(),
            payload,
            strategy.unwrap_or(default_strategy),
            author.clone(),
            signature,
        );

        self.integrate_entry(entry, true).map(|_| id)
    }

    pub fn integrate_remote(&mut self, entry: CommandEntry) -> Result<bool, CommandLogError> {
        self.lamport_clock = self.lamport_clock.max(entry.id.lamport());

        let definition = self
            .registry
            .definition(&entry.payload.command_type)
            .ok_or_else(|| {
                CommandLogError::UnregisteredCommand(entry.payload.command_type.clone())
            })?;

        if !entry.author.role.allows(definition.required_role()) {
            return Err(CommandLogError::InsufficientPermissions {
                required: definition.required_role(),
                actual: entry.author.role,
            });
        }

        if definition.require_signature() {
            let signature = entry.signature.as_ref().ok_or_else(|| {
                CommandLogError::SignatureMissing(entry.payload.command_type.clone())
            })?;
            if !self
                .verifier
                .verify(&entry.author, entry.id.lamport(), &entry.payload, signature)
            {
                return Err(CommandLogError::InvalidSignature(entry.author.id.clone()));
            }
        }

        self.integrate_entry(entry, false)
    }

    fn integrate_entry(
        &mut self,
        entry: CommandEntry,
        local: bool,
    ) -> Result<bool, CommandLogError> {
        if self.entries.contains_key(&entry.id) {
            if local {
                return Err(CommandLogError::Duplicate);
            }
            return Ok(false);
        }

        match entry.strategy {
            ConflictStrategy::Merge => {
                self.entries.insert(entry.id.clone(), entry);
                Ok(true)
            }
            ConflictStrategy::Reject => {
                let scope_key = entry.payload.scope.key();
                if self.latest_by_scope.contains_key(&scope_key) {
                    Err(CommandLogError::ConflictRejected)
                } else {
                    self.latest_by_scope.insert(scope_key, entry.id.clone());
                    self.entries.insert(entry.id.clone(), entry);
                    Ok(true)
                }
            }
            ConflictStrategy::LastWriteWins => {
                let scope_key = entry.payload.scope.key();
                if let Some(previous_id) = self.latest_by_scope.get(&scope_key).cloned() {
                    if previous_id < entry.id {
                        self.entries.remove(&previous_id);
                        self.latest_by_scope.insert(scope_key, entry.id.clone());
                        self.entries.insert(entry.id.clone(), entry);
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else {
                    self.latest_by_scope.insert(scope_key, entry.id.clone());
                    self.entries.insert(entry.id.clone(), entry);
                    Ok(true)
                }
            }
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &CommandEntry> {
        self.entries.values()
    }
}

#[derive(Default)]
pub struct NoopSignatureVerifier;

impl SignatureVerifier for NoopSignatureVerifier {
    fn verify(
        &self,
        _author: &CommandAuthor,
        _lamport: u64,
        _payload: &CommandPayload,
        _signature: &CommandSignature,
    ) -> bool {
        true
    }
}

pub struct NoopCommandSigner {
    author: CommandAuthor,
}

impl NoopCommandSigner {
    pub fn new(author: CommandAuthor) -> Self {
        Self { author }
    }
}

impl CommandSigner for NoopCommandSigner {
    fn author(&self) -> &CommandAuthor {
        &self.author
    }

    fn sign(&self, _lamport: u64, _payload: &CommandPayload) -> Option<CommandSignature> {
        None
    }
}

fn current_time_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(feature = "network-quic")]
fn signing_message(lamport: u64, payload: &CommandPayload) -> Vec<u8> {
    #[derive(Serialize)]
    struct SigningPacket<'a> {
        lamport: u64,
        #[serde(borrow)]
        payload: &'a CommandPayload,
    }

    serde_json::to_vec(&SigningPacket { lamport, payload }).unwrap_or_default()
}

#[cfg(feature = "network-quic")]
pub struct Ed25519SignatureVerifier;

#[cfg(feature = "network-quic")]
impl SignatureVerifier for Ed25519SignatureVerifier {
    fn verify(
        &self,
        author: &CommandAuthor,
        lamport: u64,
        payload: &CommandPayload,
        signature: &CommandSignature,
    ) -> bool {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let key_bytes: [u8; 32] = match author
            .public_key
            .as_ref()
            .and_then(|k| k.0.clone().try_into().ok())
        {
            Some(bytes) => bytes,
            None => return false,
        };

        let verifying_key = match VerifyingKey::from_bytes(&key_bytes) {
            Ok(key) => key,
            Err(_) => return false,
        };

        let signature_bytes: [u8; 64] = match signature.0.clone().try_into().ok() {
            Some(bytes) => bytes,
            None => return false,
        };

        let signature = match Signature::from_bytes(&signature_bytes) {
            Ok(sig) => sig,
            Err(_) => return false,
        };

        let message = signing_message(lamport, payload);
        verifying_key.verify(&message, &signature).is_ok()
    }
}

#[cfg(feature = "network-quic")]
pub struct Ed25519CommandSigner {
    author: CommandAuthor,
    keypair: ed25519_dalek::SigningKey,
}

#[cfg(feature = "network-quic")]
impl Ed25519CommandSigner {
    pub fn new(author: CommandAuthor, keypair: ed25519_dalek::SigningKey) -> Self {
        Self { author, keypair }
    }
}

#[cfg(feature = "network-quic")]
impl CommandSigner for Ed25519CommandSigner {
    fn author(&self) -> &CommandAuthor {
        &self.author
    }

    fn sign(&self, lamport: u64, payload: &CommandPayload) -> Option<CommandSignature> {
        use ed25519_dalek::Signer;

        let message = signing_message(lamport, payload);
        let signature = self.keypair.sign(&message);
        Some(CommandSignature(signature.to_bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_registry() -> Arc<CommandRegistry> {
        let mut registry = CommandRegistry::new();
        registry.register(
            "editor.selection",
            CommandDefinition::builder()
                .required_role(CommandRole::Editor)
                .default_strategy(ConflictStrategy::LastWriteWins)
                .require_signature(false)
                .build(),
        );
        registry.register(
            "editor.create",
            CommandDefinition::builder()
                .required_role(CommandRole::Editor)
                .default_strategy(ConflictStrategy::Merge)
                .require_signature(true)
                .build(),
        );
        Arc::new(registry)
    }

    struct FakeSignatureSigner {
        author: CommandAuthor,
    }

    impl FakeSignatureSigner {
        fn new(author: CommandAuthor) -> Self {
            Self { author }
        }
    }

    impl CommandSigner for FakeSignatureSigner {
        fn author(&self) -> &CommandAuthor {
            &self.author
        }

        fn sign(&self, _lamport: u64, _payload: &CommandPayload) -> Option<CommandSignature> {
            Some(CommandSignature(vec![0u8; 64]))
        }
    }

    #[test]
    fn append_local_respects_permissions() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier::default()) as Arc<dyn SignatureVerifier>;
        let mut log = CommandLog::new(registry.clone(), verifier);

        let author = CommandAuthor::new(AuthorId(1), CommandRole::Viewer);
        let signer = NoopCommandSigner::new(author);
        let payload = CommandPayload::new("editor.selection", CommandScope::Global, vec![1, 2, 3]);

        let result = log.append_local(&signer, payload, None);
        assert!(matches!(
            result,
            Err(CommandLogError::InsufficientPermissions { .. })
        ));
    }

    #[test]
    fn last_write_wins_keeps_latest_lamport() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier::default()) as Arc<dyn SignatureVerifier>;
        let mut log = CommandLog::new(registry.clone(), verifier);

        let editor = CommandAuthor::new(AuthorId(7), CommandRole::Editor);
        let signer = NoopCommandSigner::new(editor);

        let payload_one = CommandPayload::new(
            "editor.selection",
            CommandScope::Entity(EntityHandle {
                index: 1,
                generation: 0,
            }),
            vec![1],
        );
        let id_one = log
            .append_local(&signer, payload_one, None)
            .expect("first ok");

        let payload_two = CommandPayload::new(
            "editor.selection",
            CommandScope::Entity(EntityHandle {
                index: 1,
                generation: 0,
            }),
            vec![2],
        );
        let id_two = log
            .append_local(&signer, payload_two, None)
            .expect("second ok");

        assert!(id_two > id_one);
        let entries: Vec<_> = log.entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, id_two);
    }

    #[test]
    fn reject_conflict_prevents_duplicates() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier::default()) as Arc<dyn SignatureVerifier>;
        let mut log = CommandLog::new(registry.clone(), verifier);

        let editor = CommandAuthor::new(AuthorId(3), CommandRole::Editor);
        let signer = NoopCommandSigner::new(editor);

        let payload = CommandPayload::new("editor.selection", CommandScope::Global, vec![9]);
        let id = log
            .append_local(&signer, payload, Some(ConflictStrategy::Reject))
            .expect("first succeeds");

        assert_eq!(id.lamport(), 1);

        let payload_second =
            CommandPayload::new("editor.selection", CommandScope::Global, vec![10]);
        let result = log.append_local(&signer, payload_second, Some(ConflictStrategy::Reject));
        assert!(matches!(result, Err(CommandLogError::ConflictRejected)));
    }

    #[test]
    fn merge_allows_multiple_entries() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier::default()) as Arc<dyn SignatureVerifier>;
        let mut log = CommandLog::new(registry.clone(), verifier);

        let editor = CommandAuthor::new(AuthorId(4), CommandRole::Editor);
        let signer = FakeSignatureSigner::new(editor);

        for value in 0..3u8 {
            let payload = CommandPayload::new("editor.create", CommandScope::Global, vec![value]);
            log.append_local(&signer, payload, Some(ConflictStrategy::Merge))
                .expect("merge ok");
        }

        let entries: Vec<_> = log.entries().collect();
        assert_eq!(entries.len(), 3);
        let payloads: Vec<_> = entries.iter().map(|e| e.payload.data.clone()).collect();
        assert_eq!(payloads, vec![vec![0], vec![1], vec![2]]);
    }
}
