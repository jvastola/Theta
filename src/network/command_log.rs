use crate::network::EntityHandle;
use serde::{Deserialize, Serialize};
use serde_json::Error as JsonError;
use std::collections::{BTreeMap, HashMap};
#[cfg(feature = "command-log-persistence")]
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ConflictStrategy {
    #[default]
    LastWriteWins,
    Merge,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthorPublicKey(pub Vec<u8>);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

pub trait CommandSigner: Send + Sync {
    fn author(&self) -> &CommandAuthor;
    fn sign(&self, lamport: u64, payload: &CommandPayload) -> Option<CommandSignature>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[error("failed to decode command packet: {0}")]
    PacketDecodeFailed(String),
    #[error("command replay detected for author {0:?}")]
    ReplayDetected(AuthorId),
    #[error("rate limited command for author {0:?}")]
    RateLimited(AuthorId),
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub burst: u32,
    pub sustain_per_second: u32,
    pub min_refill_interval: Duration,
}

impl RateLimitConfig {
    pub fn new(burst: u32, sustain_per_second: u32, min_refill_interval: Duration) -> Self {
        let safe_burst = burst.max(1);
        let safe_interval = if min_refill_interval.is_zero() {
            Duration::from_millis(1)
        } else {
            min_refill_interval
        };
        Self {
            burst: safe_burst,
            sustain_per_second,
            min_refill_interval: safe_interval,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self::new(100, 10, Duration::from_millis(100))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CommandLogConfig {
    pub rate_limit: RateLimitConfig,
    #[cfg(feature = "command-log-persistence")]
    pub persistence: ReplayPersistenceConfig,
}

impl CommandLogConfig {
    pub fn security_defaults() -> Self {
        Self::default()
    }

    pub fn with_rate_limit(rate_limit: RateLimitConfig) -> Self {
        Self {
            rate_limit,
            #[cfg(feature = "command-log-persistence")]
            persistence: ReplayPersistenceConfig::default(),
        }
    }
}

#[cfg(feature = "command-log-persistence")]
#[derive(Clone)]
pub struct ReplayPersistenceConfig {
    handle: Option<Arc<dyn ReplayPersistence>>,
}

#[cfg(feature = "command-log-persistence")]
impl ReplayPersistenceConfig {
    pub fn disabled() -> Self {
        Self { handle: None }
    }

    pub fn with_handle(handle: Arc<dyn ReplayPersistence>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    pub fn handle(&self) -> Option<Arc<dyn ReplayPersistence>> {
        self.handle.clone()
    }
}

#[cfg(feature = "command-log-persistence")]
impl Default for ReplayPersistenceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(feature = "command-log-persistence")]
impl fmt::Debug for ReplayPersistenceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayPersistenceConfig")
            .field("enabled", &self.handle.is_some())
            .finish()
    }
}

#[cfg(feature = "command-log-persistence")]
pub trait ReplayPersistence: Send + Sync {
    fn load(&self, author: &AuthorId) -> Option<u64>;
    fn store(&self, author: &AuthorId, nonce: u64);
}

#[allow(dead_code)]
#[derive(Default)]
struct ReplayTracker {
    high_water: HashMap<AuthorId, u64>,
    #[cfg(feature = "command-log-persistence")]
    persistence: Option<Arc<dyn ReplayPersistence>>,
}

#[allow(dead_code)]
impl ReplayTracker {
    fn new() -> Self {
        Self {
            high_water: HashMap::new(),
            #[cfg(feature = "command-log-persistence")]
            persistence: None,
        }
    }

    fn from_config(config: &CommandLogConfig) -> Self {
        #[cfg(feature = "command-log-persistence")]
        {
            let mut tracker = Self::new();
            tracker.persistence = config.persistence.handle();
            tracker
        }
        #[cfg(not(feature = "command-log-persistence"))]
        {
            let _ = config;
            Self::new()
        }
    }

    fn high_water(&mut self, author: &AuthorId) -> Option<u64> {
        if let Some(value) = self.high_water.get(author).copied() {
            return Some(value);
        }
        #[cfg(feature = "command-log-persistence")]
        if let Some(persistence) = self.persistence.as_ref() {
            if let Some(stored) = persistence.load(author) {
                self.high_water.insert(author.clone(), stored);
                return Some(stored);
            }
        }
        None
    }

    fn accept_remote(&mut self, author: &AuthorId, nonce: u64) -> bool {
        match self.high_water(author) {
            Some(previous) => {
                if Self::is_newer(previous, nonce) {
                    self.store_high_water(author, nonce);
                    true
                } else {
                    false
                }
            }
            None => {
                self.store_high_water(author, nonce);
                true
            }
        }
    }

    fn record_local(&mut self, author: &AuthorId, nonce: u64) {
        self.store_high_water(author, nonce);
    }

    fn store_high_water(&mut self, author: &AuthorId, nonce: u64) {
        self.high_water.insert(author.clone(), nonce);
        #[cfg(feature = "command-log-persistence")]
        if let Some(persistence) = self.persistence.as_ref() {
            persistence.store(author, nonce);
        }
    }

    fn is_newer(previous: u64, candidate: u64) -> bool {
        if candidate == previous {
            return false;
        }
        if previous == u64::MAX && candidate == 0 {
            return true;
        }
        candidate > previous
    }
}

#[allow(dead_code)]
struct RateLimiterMap {
    buckets: HashMap<AuthorId, TokenBucket>,
    config: RateLimitConfig,
}

#[allow(dead_code)]
impl RateLimiterMap {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: HashMap::new(),
            config,
        }
    }

    fn take(&mut self, author: &AuthorId, amount: u32) -> bool {
        self.take_at(author, amount, Instant::now())
    }

    fn take_at(&mut self, author: &AuthorId, amount: u32, now: Instant) -> bool {
        let bucket = self
            .buckets
            .entry(author.clone())
            .or_insert_with(|| TokenBucket::new(&self.config, now));
        bucket.take(now, amount, &self.config)
    }

    #[cfg(test)]
    fn tokens_for(&mut self, author: &AuthorId, now: Instant) -> f64 {
        let bucket = self
            .buckets
            .entry(author.clone())
            .or_insert_with(|| TokenBucket::new(&self.config, now));
        bucket.refill(now, &self.config);
        bucket.tokens
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    last_refill: Instant,
}

#[allow(dead_code)]
impl TokenBucket {
    fn new(config: &RateLimitConfig, now: Instant) -> Self {
        let capacity = config.burst as f64;
        Self {
            capacity,
            tokens: capacity,
            last_refill: now,
        }
    }

    fn refill(&mut self, now: Instant, config: &RateLimitConfig) {
        if now <= self.last_refill {
            return;
        }
        let elapsed = now.duration_since(self.last_refill);
        if elapsed < config.min_refill_interval {
            return;
        }
        let added = config.sustain_per_second as f64 * elapsed.as_secs_f64();
        self.tokens = (self.tokens + added).min(self.capacity);
        self.last_refill = now;
    }

    fn take(&mut self, now: Instant, amount: u32, config: &RateLimitConfig) -> bool {
        self.refill(now, config);
        let demand = amount as f64;
        if self.tokens >= demand {
            self.tokens = (self.tokens - demand).max(0.0);
            true
        } else {
            false
        }
    }
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
    #[allow(dead_code)]
    config: CommandLogConfig,
    #[allow(dead_code)]
    replay_tracker: ReplayTracker,
    #[allow(dead_code)]
    rate_limiter: RateLimiterMap,
}

impl CommandLog {
    pub fn new(registry: Arc<CommandRegistry>, verifier: Arc<dyn SignatureVerifier>) -> Self {
        Self::with_config(registry, verifier, CommandLogConfig::security_defaults())
    }

    pub fn with_config(
        registry: Arc<CommandRegistry>,
        verifier: Arc<dyn SignatureVerifier>,
        config: CommandLogConfig,
    ) -> Self {
        let replay_tracker = ReplayTracker::from_config(&config);
        let rate_limiter = RateLimiterMap::new(config.rate_limit.clone());
        Self {
            lamport_clock: 0,
            entries: BTreeMap::new(),
            latest_by_scope: HashMap::new(),
            registry,
            verifier,
            config,
            replay_tracker,
            rate_limiter,
        }
    }

    pub fn set_verifier(&mut self, verifier: Arc<dyn SignatureVerifier>) {
        self.verifier = verifier;
    }

    pub fn config(&self) -> &CommandLogConfig {
        &self.config
    }

    pub fn lamport(&self) -> u64 {
        self.lamport_clock
    }

    fn next_lamport(&mut self) -> u64 {
        self.lamport_clock = self.lamport_clock.wrapping_add(1);
        self.lamport_clock
    }

    pub fn append_local(
        &mut self,
        signer: &dyn CommandSigner,
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

        if !self.rate_limiter.take(&author.id, 1) {
            return Err(CommandLogError::RateLimited(author.id.clone()));
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

        match self.integrate_entry(entry, true) {
            Ok(true) => {
                self.replay_tracker.record_local(&author.id, lamport);
                Ok(id)
            }
            Ok(false) => Ok(id),
            Err(err) => Err(err),
        }
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

        if self.entries.contains_key(&entry.id) {
            return Ok(false);
        }

        if !self.rate_limiter.take(&entry.author.id, 1) {
            return Err(CommandLogError::RateLimited(entry.author.id.clone()));
        }

        if !self
            .replay_tracker
            .accept_remote(&entry.author.id, entry.id.lamport())
        {
            return Err(CommandLogError::ReplayDetected(entry.author.id.clone()));
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
                match self.latest_by_scope.entry(scope_key) {
                    std::collections::hash_map::Entry::Occupied(_) => {
                        Err(CommandLogError::ConflictRejected)
                    }
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(entry.id.clone());
                        self.entries.insert(entry.id.clone(), entry);
                        Ok(true)
                    }
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

    pub fn entry(&self, id: &CommandId) -> Option<&CommandEntry> {
        self.entries.get(id)
    }

    pub fn entries_since(&self, last: Option<&CommandId>) -> Vec<CommandEntry> {
        let mut results = Vec::new();
        for (id, entry) in &self.entries {
            if last.is_some_and(|prev| id <= prev) {
                continue;
            }
            results.push(entry.clone());
        }
        results
    }

    pub fn latest_id(&self) -> Option<CommandId> {
        self.entries.keys().next_back().cloned()
    }

    pub fn integrate_batch(&mut self, batch: &CommandBatch) -> Result<usize, CommandLogError> {
        let mut applied = 0;
        for entry in &batch.entries {
            if self.integrate_remote(entry.clone())? {
                applied += 1;
            }
        }
        Ok(applied)
    }

    pub fn integrate_packet(&mut self, packet: &CommandPacket) -> Result<usize, CommandLogError> {
        let batch = packet
            .decode()
            .map_err(|err| CommandLogError::PacketDecodeFailed(err.to_string()))?;
        self.integrate_batch(&batch)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandBatch {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub entries: Vec<CommandEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandPacket {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub payload: Vec<u8>,
}

impl CommandPacket {
    pub fn from_batch(batch: &CommandBatch) -> Result<Self, JsonError> {
        Ok(Self {
            sequence: batch.sequence,
            timestamp_ms: batch.timestamp_ms,
            payload: serde_json::to_vec(batch)?,
        })
    }

    /// Decodes the payload into a CommandBatch.
    /// Note: The sequence and timestamp fields are present both in the packet and the payload.
    /// This method trusts the values from the payload for consistency, but you may wish to validate
    /// or reconcile these fields if you expect them to differ.
    pub fn decode(&self) -> Result<CommandBatch, JsonError> {
        let batch: CommandBatch = serde_json::from_slice(&self.payload)?;
        // If you want to ensure consistency, you could assert or compare the fields here.
        // For now, we return the batch as deserialized.
        Ok(batch)
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

        let message = signing_message(lamport, payload);
        match Signature::try_from(signature_bytes.as_slice()) {
            Ok(sig) => verifying_key.verify(&message, &sig).is_ok(),
            Err(_) => false,
        }
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
    use proptest::prelude::*;
    use std::time::{Duration, Instant};

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
    fn command_log_config_defaults_align_with_security_expectations() {
        let defaults = CommandLogConfig::security_defaults();
        assert_eq!(defaults.rate_limit.burst, 100);
        assert_eq!(defaults.rate_limit.sustain_per_second, 10);
        assert!(defaults.rate_limit.min_refill_interval >= Duration::from_millis(1));
    }

    #[test]
    fn replay_tracker_tracks_high_water_mark() {
        let config = CommandLogConfig::security_defaults();
        let mut tracker = ReplayTracker::from_config(&config);
        let author = AuthorId(11);

        assert_eq!(tracker.high_water(&author), None);
        tracker.record_local(&author, 5);
        assert_eq!(tracker.high_water(&author), Some(5));
        assert!(!tracker.accept_remote(&author, 4));
        assert!(tracker.accept_remote(&author, 6));
        assert_eq!(tracker.high_water(&author), Some(6));
        tracker.record_local(&author, u64::MAX);
        assert!(tracker.accept_remote(&author, 0));
        assert_eq!(tracker.high_water(&author), Some(0));
    }

    #[test]
    fn rate_limiter_enforces_burst_and_refills() {
        let config = RateLimitConfig::new(2, 200, Duration::from_millis(1));
        let mut limiter = RateLimiterMap::new(config);
        let author = AuthorId(99);
        let now = Instant::now();

        assert!(limiter.take_at(&author, 1, now));
        assert!(limiter.take_at(&author, 1, now));
        assert!(!limiter.take_at(&author, 1, now));

        let later = now + Duration::from_millis(5);
        assert!(limiter.take_at(&author, 1, later));
    }

    #[test]
    fn append_local_respects_rate_limiter() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
        let config =
            CommandLogConfig::with_rate_limit(RateLimitConfig::new(1, 0, Duration::from_secs(1)));
        let mut log = CommandLog::with_config(Arc::clone(&registry), verifier, config);

        let editor = CommandAuthor::new(AuthorId(21), CommandRole::Editor);
        let signer = NoopCommandSigner::new(editor);

        let payload = CommandPayload::new("editor.selection", CommandScope::Global, vec![1]);
        log.append_local(&signer, payload, None)
            .expect("first append succeeds");

        let payload_second = CommandPayload::new("editor.selection", CommandScope::Global, vec![2]);
        let err = log
            .append_local(&signer, payload_second, None)
            .expect_err("second append should rate limit");
        assert!(matches!(err, CommandLogError::RateLimited(AuthorId(21))));
    }

    #[test]
    fn integrate_remote_rejects_stale_entries() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
        let mut source = CommandLog::new(Arc::clone(&registry), Arc::clone(&verifier));
        let mut sink = CommandLog::new(Arc::clone(&registry), verifier);

        let author = CommandAuthor::new(AuthorId(31), CommandRole::Editor);
        let signer = FakeSignatureSigner::new(author.clone());

        let payload = CommandPayload::new("editor.create", CommandScope::Global, vec![9]);
        source
            .append_local(&signer, payload, Some(ConflictStrategy::Merge))
            .expect("append");

        let entry = source.entries().next().expect("entry").clone();
        assert!(sink.integrate_remote(entry.clone()).expect("first ok"));

        let stale_entry = CommandEntry::new(
            CommandId::new(entry.id.lamport() - 1, author.id.clone()),
            entry.timestamp_ms,
            entry.payload.clone(),
            entry.strategy,
            author,
            entry.signature.clone(),
        );

        let err = sink
            .integrate_remote(stale_entry)
            .expect_err("stale entry rejected");
        assert!(matches!(err, CommandLogError::ReplayDetected(AuthorId(31))));
    }

    #[test]
    fn entries_since_tracks_latest_id() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
        let mut log = CommandLog::new(registry.clone(), verifier);

        let editor = CommandAuthor::new(AuthorId(9), CommandRole::Editor);
        let signer = FakeSignatureSigner::new(editor);

        let first_payload = CommandPayload::new("editor.create", CommandScope::Global, vec![0]);
        let first_id = log
            .append_local(&signer, first_payload, Some(ConflictStrategy::Merge))
            .expect("append first");

        let initial = log.entries_since(None);
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].id, first_id);

        let second_payload = CommandPayload::new("editor.create", CommandScope::Global, vec![1]);
        let second_id = log
            .append_local(&signer, second_payload, Some(ConflictStrategy::Merge))
            .expect("append second");

        let delta = log.entries_since(Some(&first_id));
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].id, second_id);
        assert_eq!(log.latest_id(), Some(second_id));
    }

    #[test]
    fn append_local_respects_permissions() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
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
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
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
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
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
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
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

    #[test]
    fn integrate_batch_replays_entries() {
        let registry = setup_registry();
        let verifier = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
        let mut local = CommandLog::new(Arc::clone(&registry), Arc::clone(&verifier));
        let mut remote = CommandLog::new(registry, verifier);

        let editor = CommandAuthor::new(AuthorId(10), CommandRole::Editor);
        let signer = FakeSignatureSigner::new(editor.clone());

        for value in 0..3u8 {
            let payload = CommandPayload::new(
                "editor.create",
                CommandScope::Tool("brush".to_string()),
                vec![value],
            );
            local
                .append_local(&signer, payload, Some(ConflictStrategy::Merge))
                .expect("append");
        }

        let entries: Vec<_> = local.entries().cloned().collect();
        let batch = CommandBatch {
            sequence: 1,
            timestamp_ms: 123,
            entries,
        };

        let applied = remote.integrate_batch(&batch).expect("replay batch");
        assert_eq!(applied, 3);
        let remote_entries: Vec<_> = remote.entries().cloned().collect();
        assert_eq!(remote_entries.len(), 3);
    }

    proptest::proptest! {
        #[test]
        fn replay_fuzz_matches_direct_application(ops in proptest::collection::vec(
            (
                proptest::num::u16::ANY,
                proptest::num::u8::ANY,
                proptest::num::u8::ANY,
                proptest::bool::ANY,
            ),
            1..48
        )) {
            let registry = setup_registry();
            let verifier_local = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
            let verifier_remote = Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>;
            let mut local = CommandLog::new(Arc::clone(&registry), verifier_local);
            let mut remote = CommandLog::new(Arc::clone(&registry), verifier_remote);
            let mut replay = CommandLog::new(Arc::clone(&registry), Arc::new(NoopSignatureVerifier) as Arc<dyn SignatureVerifier>);

            let author = CommandAuthor::new(AuthorId(42), CommandRole::Editor);
            let signer = FakeSignatureSigner::new(author);
            let mut last_id: Option<CommandId> = None;
            let mut batches = Vec::new();

            for (scope_seed, payload_seed, strategy_seed, tool_scope) in ops {
                let scope = if tool_scope {
                    CommandScope::Tool(format!("tool-{}", scope_seed % 5))
                } else {
                    CommandScope::Entity(EntityHandle {
                        index: (scope_seed % 16) as u32,
                        generation: (scope_seed % 3) as u32,
                    })
                };

                let strategy = match strategy_seed % 3 {
                    0 => ConflictStrategy::LastWriteWins,
                    1 => ConflictStrategy::Merge,
                    _ => ConflictStrategy::LastWriteWins,
                };

                let payload_bytes = vec![payload_seed];
                let command_type = if strategy_seed % 2 == 0 {
                    "editor.selection"
                } else {
                    "editor.create"
                };

                let payload = CommandPayload::new(command_type, scope, payload_bytes);
                if local.append_local(&signer, payload, Some(strategy)).is_ok() {
                    let new_entries = local.entries_since(last_id.as_ref());
                    if !new_entries.is_empty() {
                        let batch = CommandBatch {
                            sequence: batches.len() as u64 + 1,
                            timestamp_ms: batches.len() as u64 + 100,
                            entries: new_entries.clone(),
                        };
                        remote.integrate_batch(&batch).expect("batch replay");
                        let packet = CommandPacket::from_batch(&batch).expect("packet serialize");
                        replay.integrate_packet(&packet).expect("packet replay");
                        batches.push(batch);
                        last_id = local.latest_id();
                    }
                }
            }

            let local_entries: Vec<_> = local.entries().cloned().collect();
            let remote_entries: Vec<_> = remote.entries().cloned().collect();
            let replay_entries: Vec<_> = replay.entries().cloned().collect();

            let baseline = local_entries.clone();
            prop_assert_eq!(remote_entries, baseline);
            prop_assert_eq!(replay_entries, local_entries);
        }
    }
}
