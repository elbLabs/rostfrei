use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_messaging_core::BoundedContextName;
use thiserror::Error;

use crate::{
    CompletedOutcome, EncodedOutcomeDetails, EventHistory, EventStoreError, EventTransaction,
    OperationId,
};

/// Configured storage authority/incarnation, never derived from a caller's principal or scope.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandAuthority(String);

impl CommandAuthority {
    pub(crate) fn memory(incarnation: u64) -> Self {
        Self(format!("memory:{}:{incarnation}", std::process::id()))
    }
    pub fn new(value: impl Into<String>) -> Result<Self, crate::IdentityError> {
        let value = value.into();
        crate::identity::validate(&value, "command authority", 256)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutcomeKey {
    authority: CommandAuthority,
    context: BoundedContextName,
    operation: OperationId,
}

impl OutcomeKey {
    pub const fn new(
        authority: CommandAuthority,
        context: BoundedContextName,
        operation: OperationId,
    ) -> Self {
        Self {
            authority,
            context,
            operation,
        }
    }

    pub const fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    pub const fn bounded_context(&self) -> &BoundedContextName {
        &self.context
    }
    pub const fn operation_id(&self) -> &OperationId {
        &self.operation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutcomeLookup<T> {
    Missing,
    Completed(CompletedOutcome<T>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletedCommit {
    Appended(CompletedOutcome<EncodedOutcomeDetails>),
    ExactReplay(CompletedOutcome<EncodedOutcomeDetails>),
}

impl CompletedCommit {
    pub const fn outcome(&self) -> &CompletedOutcome<EncodedOutcomeDetails> {
        match self {
            Self::Appended(value) | Self::ExactReplay(value) => value,
        }
    }
    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

#[async_trait]
pub trait OutcomeReader: Send + Sync {
    /// Only verified absence in this authority permits command evaluation.
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError>;
}

/// One coordinator commits events, read guards and the permanent winning marker.
#[async_trait]
pub trait CommandBackend: EventHistory + OutcomeReader {
    fn authority(&self) -> &CommandAuthority;
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError>;
}

#[async_trait]
pub trait CommandCommitSession: EventHistory {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError>;
}

/// Opaque token bound to an immutable winning reference, including authority incarnation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutcomeRevision(pub(crate) crate::ContentFingerprint);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReclamationStatus {
    Pending,
    Reclaimed,
}

/// A proven logical withdrawal, with separately reported retained-payload reclamation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeExpiry {
    revision: OutcomeRevision,
    reclamation: ReclamationStatus,
}
impl OutcomeExpiry {
    pub const fn new(revision: OutcomeRevision, reclamation: ReclamationStatus) -> Self {
        Self {
            revision,
            reclamation,
        }
    }
    pub const fn revision(&self) -> &OutcomeRevision {
        &self.revision
    }
    pub const fn reclamation(&self) -> ReclamationStatus {
        self.reclamation
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum OutcomeLifecycleError {
    #[error("outcome does not exist")]
    Missing,
    #[error("revision does not identify this winning outcome")]
    RevisionMismatch,
    #[error("this outcome representation does not support expiry")]
    Unsupported,
    #[error(transparent)]
    Store(#[from] EventStoreError),
}

#[async_trait]
pub trait OutcomeLifecycle: Send + Sync {
    /// Privileged maintenance. Success proves withdrawal; reclamation is reported separately.
    async fn expire_details(
        &self,
        key: &OutcomeKey,
        expected: &OutcomeRevision,
    ) -> Result<OutcomeExpiry, OutcomeLifecycleError>;
}

#[async_trait]
impl<T: OutcomeReader + ?Sized> OutcomeReader for Arc<T> {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.as_ref().lookup_outcome(key).await
    }
}

#[async_trait]
impl<T: CommandBackend + ?Sized> CommandBackend for Arc<T> {
    fn authority(&self) -> &CommandAuthority {
        self.as_ref().authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.as_ref().begin_command().await
    }
}
