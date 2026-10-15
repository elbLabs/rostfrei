//! Permanent decision evidence and independently recoverable, explicitly withdrawable details.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    CommandAuthority, CommandCompletion, CommandExecutionMetadata, ContentFingerprint,
    EventStoreError, EventStoreErrorKind, EventTransaction, InlineCompletedOutcome,
    OutcomeEvidence, OutcomeKey, OutcomeRevision, ProposalDigest, SecurityScope,
    TransactionReceipt,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OutcomeDecision {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutcomeDetailReference {
    key: OutcomeKey,
    security_scope: Option<SecurityScope>,
    decision: OutcomeDecision,
    format: u16,
    length: usize,
    digest: ContentFingerprint,
}

impl OutcomeDetailReference {
    pub const fn key(&self) -> &OutcomeKey {
        &self.key
    }
    pub const fn security_scope(&self) -> Option<&SecurityScope> {
        self.security_scope.as_ref()
    }
    pub const fn decision(&self) -> OutcomeDecision {
        self.decision
    }
    pub const fn format(&self) -> u16 {
        self.format
    }
    pub const fn length(&self) -> usize {
        self.length
    }
    pub const fn digest(&self) -> ContentFingerprint {
        self.digest
    }
}

/// No command/rejection body. The receipt contains retained event-integrity material only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeMarker {
    reference: OutcomeDetailReference,
    revision: OutcomeRevision,
    proposal: ProposalDigest,
    evidence: Option<OutcomeEvidence>,
    integrity: TransactionReceipt,
}

impl OutcomeMarker {
    /// Creates the marker/detail pair before the coordinator's atomic decision.
    /// This function alone does not confer commit authority.
    pub fn prepare(
        authority: CommandAuthority,
        proposal: &EventTransaction,
        receipt: TransactionReceipt,
    ) -> Result<(Self, Vec<u8>), EventStoreError> {
        if proposal.completion().is_some() {
            crate::validate_completed_transaction(proposal)?;
        }
        if receipt.bounded_context() != proposal.bounded_context()
            || receipt.operation_id() != proposal.operation_id()
            || receipt.operation_fingerprint() != proposal.operation_fingerprint()
            || receipt.correlation_id() != proposal.correlation_id()
            || receipt.causation_id() != proposal.causation_id()
            || receipt.security_scope() != proposal.security_scope()
            || receipt
                .completion()
                .is_some_and(|value| Some(value) != proposal.completion())
            || (proposal
                .completion()
                .is_some_and(|value| value.rejection.is_some())
                && !receipt.events().is_empty())
        {
            return Err(EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "outcome integrity receipt does not match its proposal",
            ));
        }
        let context = proposal.bounded_context().ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "outcome requires context",
            )
        })?;
        let key = OutcomeKey::new(authority, context.clone(), proposal.operation_id().clone());
        let details = EncodedOutcomeDetails {
            completion: proposal.completion().cloned(),
        };
        let bytes = details.encode(&key, proposal.security_scope())?;
        let reference = OutcomeDetailReference {
            key,
            security_scope: proposal.security_scope().cloned(),
            decision: if details.rejection().is_some() {
                OutcomeDecision::Rejected
            } else {
                OutcomeDecision::Accepted
            },
            format: 1,
            length: bytes.len(),
            digest: ContentFingerprint::digest(&bytes),
        };
        let proposal = ProposalDigest::of(proposal)?;
        let mut hash = Sha256::new();
        crate::identity::hash_part(&mut hash, b"rostfrei:outcome-revision:v1");
        crate::identity::hash_part(&mut hash, reference.digest.as_bytes());
        crate::identity::hash_part(&mut hash, proposal.fingerprint().as_bytes());
        let revision = OutcomeRevision(ContentFingerprint::from_bytes(hash.finalize().into()));
        let evidence = details
            .completion
            .as_ref()
            .and_then(|v| v.evidence.clone())
            .map(|mut v| {
                // First timestamp is retainable detail, not command identity.
                v.created_at_millis = None;
                v
            });
        Ok((
            Self {
                reference,
                revision,
                proposal,
                evidence,
                integrity: receipt.without_completion(),
            },
            bytes,
        ))
    }

    pub const fn detail_reference(&self) -> &OutcomeDetailReference {
        &self.reference
    }
    pub const fn revision(&self) -> &OutcomeRevision {
        &self.revision
    }
    pub const fn proposal_digest(&self) -> ProposalDigest {
        self.proposal
    }
    pub const fn decision(&self) -> OutcomeDecision {
        self.reference.decision
    }
    pub const fn integrity_receipt(&self) -> &TransactionReceipt {
        &self.integrity
    }
    pub const fn evidence(&self) -> Option<&OutcomeEvidence> {
        self.evidence.as_ref()
    }

    /// Identity reconciliation deliberately ignores the speculative proposal and timestamp.
    pub fn validate_identity(
        &self,
        metadata: &CommandExecutionMetadata,
    ) -> Result<(), EventStoreError> {
        let evidence_matches = match (self.evidence(), metadata.outcome_evidence()) {
            (Some(a), Some(b)) => a.same_identity(b),
            // Legacy acceptance has no command evidence. Policy-enabled legacy scope
            // mapping must be explicit at the service/native boundary.
            (None, _) => true,
            _ => false,
        };
        let receipt = &self.integrity;
        if receipt.bounded_context() != metadata.bounded_context()
            || receipt.operation_id() != metadata.operation_id()
            || receipt.operation_fingerprint() != metadata.operation_fingerprint()
            || receipt.correlation_id() != metadata.correlation_id()
            || receipt.causation_id() != metadata.causation_id()
            || self.reference.security_scope.as_ref() != metadata.security_scope()
            || !evidence_matches
        {
            return Err(EventStoreError::new(
                EventStoreErrorKind::IdentityConflict,
                "completed command identity differs",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveReference(String);
impl ArchiveReference {
    pub fn new(value: impl Into<String>) -> Result<Self, crate::IdentityError> {
        let value = value.into();
        crate::identity::validate(&value, "archive reference", 1024)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DetailUnavailableReason {
    MissingDetails,
    BackendUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutcomeDetails<T> {
    Available(T),
    Expired,
    Archived { reference: ArchiveReference },
    Unavailable { reason: DetailUnavailableReason },
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum OutcomeRecoveryError {
    #[error("completed outcome details expired")]
    Expired,
    #[error("completed outcome is archived at {reference:?}")]
    Archived { reference: ArchiveReference },
    #[error("completed outcome details unavailable: {reason:?}")]
    Unavailable { reason: DetailUnavailableReason },
}

impl<T> OutcomeDetails<T> {
    pub fn available(&self) -> Result<&T, OutcomeRecoveryError> {
        match self {
            Self::Available(value) => Ok(value),
            Self::Expired => Err(OutcomeRecoveryError::Expired),
            Self::Archived { reference } => Err(OutcomeRecoveryError::Archived {
                reference: reference.clone(),
            }),
            Self::Unavailable { reason } => {
                Err(OutcomeRecoveryError::Unavailable { reason: *reason })
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedOutcome<T = EncodedOutcomeDetails> {
    marker: Box<OutcomeMarker>,
    details: OutcomeDetails<T>,
}

impl<T> CompletedOutcome<T> {
    pub fn new(marker: OutcomeMarker, details: OutcomeDetails<T>) -> Self {
        Self {
            marker: Box::new(marker),
            details,
        }
    }
    pub const fn marker(&self) -> &OutcomeMarker {
        &self.marker
    }
    pub const fn details(&self) -> &OutcomeDetails<T> {
        &self.details
    }
    pub fn try_map<U, E>(
        self,
        map: impl FnOnce(T) -> Result<U, E>,
    ) -> Result<CompletedOutcome<U>, E> {
        let details = match self.details {
            OutcomeDetails::Available(value) => OutcomeDetails::Available(map(value)?),
            OutcomeDetails::Expired => OutcomeDetails::Expired,
            OutcomeDetails::Archived { reference } => OutcomeDetails::Archived { reference },
            OutcomeDetails::Unavailable { reason } => OutcomeDetails::Unavailable { reason },
        };
        Ok(CompletedOutcome {
            marker: self.marker,
            details,
        })
    }
}

impl CompletedOutcome<EncodedOutcomeDetails> {
    /// Deliberate compatibility materialization. No inline receipt is stored by the split backend.
    pub fn to_inline(&self) -> Result<InlineCompletedOutcome, OutcomeRecoveryError> {
        let details = self.details.available()?;
        let mut receipt = self.marker.integrity.clone();
        if let Some(completion) = &details.completion {
            receipt = receipt.with_completion(completion.clone());
        }
        Ok(InlineCompletedOutcome::new(receipt))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedOutcomeDetails {
    completion: Option<CommandCompletion>,
}

impl EncodedOutcomeDetails {
    pub const fn completion(&self) -> Option<&CommandCompletion> {
        self.completion.as_ref()
    }
    pub fn rejection(&self) -> Option<&crate::EncodedRejection> {
        self.completion.as_ref().and_then(|v| v.rejection.as_ref())
    }

    fn encode(
        &self,
        key: &OutcomeKey,
        scope: Option<&SecurityScope>,
    ) -> Result<Vec<u8>, EventStoreError> {
        let mut completion = self.completion.clone();
        let payload = completion
            .as_mut()
            .and_then(|v| v.rejection.as_mut())
            .map_or_else(Vec::new, |r| std::mem::take(&mut r.bytes));
        let header = DetailHeader {
            version: 1,
            authority: key.authority().as_str().to_owned(),
            context: key.bounded_context().as_str().to_owned(),
            operation: key.operation_id().as_str().to_owned(),
            security_scope: scope.map(|v| v.as_str().to_owned()),
            completion,
        };
        let header = serde_json::to_vec(&header).map_err(|e| corrupt(e.to_string()))?;
        let length = u32::try_from(header.len()).map_err(|e| corrupt(e.to_string()))?;
        let mut bytes = length.to_be_bytes().to_vec();
        bytes.extend(header);
        bytes.extend(payload);
        Ok(bytes)
    }

    pub fn decode(
        reference: &OutcomeDetailReference,
        bytes: &[u8],
    ) -> Result<Self, EventStoreError> {
        if reference.format != 1
            || bytes.len() != reference.length
            || ContentFingerprint::digest(bytes) != reference.digest
        {
            return Err(corrupt("outcome detail format, length or digest mismatch"));
        }
        let (length, body) = bytes
            .split_at_checked(4)
            .ok_or_else(|| corrupt("truncated detail header"))?;
        let length = u32::from_be_bytes(
            length
                .try_into()
                .map_err(|_| corrupt("invalid detail length"))?,
        );
        let length = usize::try_from(length).map_err(|_| corrupt("detail length overflow"))?;
        let (header, payload) = body
            .split_at_checked(length)
            .ok_or_else(|| corrupt("truncated detail body"))?;
        let mut header: DetailHeader =
            serde_json::from_slice(header).map_err(|e| corrupt(e.to_string()))?;
        if header.version != 1
            || header.authority != reference.key.authority().as_str()
            || header.context != reference.key.bounded_context().as_str()
            || header.operation != reference.key.operation_id().as_str()
            || header.security_scope.as_deref()
                != reference.security_scope.as_ref().map(SecurityScope::as_str)
        {
            return Err(corrupt("outcome detail identity mismatch"));
        }
        if let Some(rejection) = header
            .completion
            .as_mut()
            .and_then(|v| v.rejection.as_mut())
        {
            if !rejection.bytes.is_empty() {
                return Err(corrupt("inline rejection in detail header"));
            }
            rejection.bytes = payload.to_vec();
        } else if !payload.is_empty() {
            return Err(corrupt("unexpected accepted detail payload"));
        }
        if let Some(completion) = &header.completion {
            completion.validate().map_err(|e| corrupt(e.to_string()))?;
        }
        let details = Self {
            completion: header.completion,
        };
        if (details.rejection().is_some()) != (reference.decision == OutcomeDecision::Rejected) {
            return Err(corrupt("outcome detail decision mismatch"));
        }
        Ok(details)
    }
}

// JSON metadata plus length-framed raw rejection bytes, not a JSON numeric byte array.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DetailHeader {
    version: u16,
    authority: String,
    context: String,
    operation: String,
    security_scope: Option<String>,
    completion: Option<CommandCompletion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DetailRead {
    Found(Vec<u8>),
    AbsentHere,
    Archived { reference: ArchiveReference },
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DetailReadError {
    #[error("detail backend unavailable")]
    Unavailable,
    #[error("detail read forbidden")]
    Forbidden,
    #[error("detail integrity failure")]
    Corrupt,
    #[error("detail backend configuration mismatch")]
    ConfigurationMismatch,
}

#[async_trait]
pub trait OutcomeDetailReader: Send + Sync {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError>;
}

/// Must be called only after this invocation's authoritative lifecycle check.
pub async fn resolve_details(
    reference: &OutcomeDetailReference,
    hot: &dyn OutcomeDetailReader,
    cold: Option<&dyn OutcomeDetailReader>,
) -> Result<OutcomeDetails<EncodedOutcomeDetails>, EventStoreError> {
    let mut unavailable = false;
    let mut archive = None;
    for reader in std::iter::once(hot).chain(cold) {
        match reader.get(reference).await {
            Ok(DetailRead::Found(bytes)) => {
                return Ok(OutcomeDetails::Available(EncodedOutcomeDetails::decode(
                    reference, &bytes,
                )?));
            }
            Ok(DetailRead::AbsentHere) => {}
            Ok(DetailRead::Archived { reference }) => archive = Some(reference),
            Err(DetailReadError::Unavailable) => unavailable = true,
            Err(error) => {
                return Err(EventStoreError::new(
                    match error {
                        DetailReadError::Forbidden => EventStoreErrorKind::DetailForbidden,
                        DetailReadError::ConfigurationMismatch => {
                            EventStoreErrorKind::ConfigurationMismatch
                        }
                        DetailReadError::Corrupt => EventStoreErrorKind::CorruptHistory,
                        DetailReadError::Unavailable => EventStoreErrorKind::Unavailable,
                    },
                    error.to_string(),
                ));
            }
        }
    }
    Ok(if unavailable {
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::BackendUnavailable,
        }
    } else if let Some(reference) = archive {
        OutcomeDetails::Archived { reference }
    } else {
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::MissingDetails,
        }
    })
}

fn corrupt(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}
