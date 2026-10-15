use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei_core::{CompletedOutcome, OutcomeLookup, SecurityScope};
use rostfrei_messaging_core::{CommandResponse, MessageId};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{CommandProcessorError, EncodedCommand};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrincipalId(String);
impl PrincipalId {
    pub fn new(value: impl Into<String>) -> Result<Self, rostfrei_core::IdentityError> {
        let value = value.into();
        SecurityScope::new(value.clone())?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Constructed by trusted ingress, never deserialized from a command or caller headers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedCommandContext {
    principal: PrincipalId,
    scope: SecurityScope,
}
impl TrustedCommandContext {
    pub const fn new(principal: PrincipalId, scope: SecurityScope) -> Self {
        Self { principal, scope }
    }
    pub const fn principal(&self) -> &PrincipalId {
        &self.principal
    }
    pub const fn security_scope(&self) -> &SecurityScope {
        &self.scope
    }
}

pub struct ValidatedCommand {
    pub(crate) original: EncodedCommand,
}
impl ValidatedCommand {
    pub const fn original(&self) -> &EncodedCommand {
        &self.original
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandAccess {
    ReadOutcome,
    Execute,
}
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AuthorizationError {
    #[error("authentication is required")]
    Unauthenticated,
    #[error("command access denied")]
    Denied,
    #[error("authorization provider unavailable")]
    Unavailable,
    #[error("authorization policy misconfigured")]
    InvalidConfiguration,
}
#[async_trait]
pub trait CommandAuthorizer: Send + Sync {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError>;
}
pub struct AllowAllCommands;
#[async_trait]
impl CommandAuthorizer for AllowAllCommands {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionOrigin {
    Committed,
    Replayed,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessedCommand {
    pub(crate) origin: CompletionOrigin,
    pub(crate) outcome: CompletedOutcome<CommandResponse>,
}
impl ProcessedCommand {
    pub const fn new(origin: CompletionOrigin, outcome: CompletedOutcome<CommandResponse>) -> Self {
        Self { origin, outcome }
    }
    pub const fn origin(&self) -> CompletionOrigin {
        self.origin
    }
    pub const fn outcome(&self) -> &CompletedOutcome<CommandResponse> {
        &self.outcome
    }
}
pub struct ServiceReport<T> {
    pub(crate) result: Result<T, CommandProcessorError>,
    pub(crate) attempt: AttemptId,
    pub(crate) audit: AuditDelivery,
}
impl<T> ServiceReport<T> {
    pub const fn new(
        result: Result<T, CommandProcessorError>,
        attempt: AttemptId,
        audit: AuditDelivery,
    ) -> Self {
        Self {
            result,
            attempt,
            audit,
        }
    }
    pub const fn result(&self) -> &Result<T, CommandProcessorError> {
        &self.result
    }
    pub fn into_result(self) -> Result<T, CommandProcessorError> {
        self.result
    }
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt
    }
    pub const fn audit(&self) -> &AuditDelivery {
        &self.audit
    }
}
#[async_trait]
pub trait CommandProcessingService: Send + Sync {
    async fn process(
        &self,
        caller: &TrustedCommandContext,
        command: &EncodedCommand,
    ) -> ServiceReport<ProcessedCommand>;
    async fn lookup(
        &self,
        caller: &TrustedCommandContext,
        command: &EncodedCommand,
    ) -> ServiceReport<OutcomeLookup<CommandResponse>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptId(String);
impl AttemptId {
    pub fn generate() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self(format!(
            "{}:{time}:{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditPhase {
    Started,
    Finished,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditTerminal {
    Committed,
    Replayed,
    Missing,
    Denied,
    Failed,
    Expired,
    Unavailable,
}
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct AuditError {
    message: String,
}
impl AuditError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandAuditRecord {
    pub(crate) attempt: AttemptId,
    pub(crate) phase: AuditPhase,
    pub(crate) terminal: Option<AuditTerminal>,
    pub(crate) command: MessageId,
    pub(crate) original: Vec<u8>,
    pub(crate) caller: TrustedCommandContext,
}
impl CommandAuditRecord {
    pub const fn attempt_id(&self) -> &AttemptId {
        &self.attempt
    }
    pub const fn phase(&self) -> AuditPhase {
        self.phase
    }
    pub const fn terminal(&self) -> Option<AuditTerminal> {
        self.terminal
    }
    pub const fn command_id(&self) -> &MessageId {
        &self.command
    }
    pub fn original_payload(&self) -> &[u8] {
        &self.original
    }
    pub const fn principal(&self) -> &PrincipalId {
        self.caller.principal()
    }
    pub const fn security_scope(&self) -> &SecurityScope {
        self.caller.security_scope()
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditDelivery {
    Disabled,
    Delivered,
    Failed {
        records: Vec<(AuditPhase, AuditError)>,
    },
}
#[async_trait]
pub trait CommandAuditSink: Send + Sync {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), AuditError>;
}
/// Bounded best-effort history. Evicts oldest records at capacity; clearing affects no authority.
pub struct InMemoryCommandAuditSink {
    records: Mutex<VecDeque<CommandAuditRecord>>,
    maximum: usize,
}
impl InMemoryCommandAuditSink {
    pub fn new(maximum: usize) -> Self {
        Self {
            records: Mutex::new(VecDeque::new()),
            maximum,
        }
    }
    pub async fn records(&self) -> Vec<CommandAuditRecord> {
        self.records.lock().await.iter().cloned().collect()
    }
    pub async fn clear(&self) {
        self.records.lock().await.clear();
    }
}
#[async_trait]
impl CommandAuditSink for InMemoryCommandAuditSink {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), AuditError> {
        if self.maximum == 0 {
            return Err(AuditError::new("audit retention capacity is zero"));
        }
        let mut records = self.records.lock().await;
        if records.len() >= self.maximum {
            records.pop_front();
        }
        records.push_back(record.clone());
        drop(records);
        Ok(())
    }
}
