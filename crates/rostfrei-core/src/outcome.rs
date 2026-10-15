//! Permanent decision evidence and independently recoverable, explicitly withdrawable details.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::any::Any;
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
    actor: Option<crate::ActorRef>,
    decision: OutcomeDecision,
    format: u16,
    length: usize,
    digest: ContentFingerprint,
}

impl OutcomeDetailReference {
    /// Canonical complete reference encoding, separate from mutable lifecycle state.
    pub fn encode(&self) -> Result<Vec<u8>, EventStoreError> {
        if let Some(actor) = &self.actor {
            return serde_json::to_vec(&(
                2_u16,
                self.key.authority().as_str(),
                self.key.bounded_context().as_str(),
                self.key.operation_id().as_str(),
                self.security_scope.as_ref().map(SecurityScope::as_str),
                actor,
                self.decision == OutcomeDecision::Rejected,
                self.format,
                self.length,
                self.digest.to_hex(),
            ))
            .map_err(|e| corrupt(e.to_string()));
        }
        serde_json::to_vec(&(
            1_u16,
            self.key.authority().as_str(),
            self.key.bounded_context().as_str(),
            self.key.operation_id().as_str(),
            self.security_scope.as_ref().map(SecurityScope::as_str),
            self.decision == OutcomeDecision::Rejected,
            self.format,
            self.length,
            self.digest.to_hex(),
        ))
        .map_err(|e| corrupt(e.to_string()))
    }
    pub const fn key(&self) -> &OutcomeKey {
        &self.key
    }
    pub const fn actor(&self) -> Option<&crate::ActorRef> {
        self.actor.as_ref()
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
    /// Versioned, payload-free storage metadata. Event integrity remains in the journal.
    pub fn encode_manifest(&self) -> Result<Vec<u8>, EventStoreError> {
        serde_json::to_vec(&MarkerManifest {
            version: self.reference.format,
            authority: self.reference.key.authority().as_str().to_owned(),
            context: self.reference.key.bounded_context().as_str().to_owned(),
            operation: self.reference.key.operation_id().as_str().to_owned(),
            scope: self
                .reference
                .security_scope
                .as_ref()
                .map(|v| v.as_str().to_owned()),
            actor: self.actor().cloned(),
            rejected: self.decision() == OutcomeDecision::Rejected,
            format: self.reference.format,
            length: self.reference.length,
            digest: self.reference.digest.to_hex(),
            proposal_version: self.proposal.version(),
            proposal: self.proposal.fingerprint().to_hex(),
            revision: self.revision.0.to_hex(),
            evidence: self.evidence.clone(),
            integrity: integrity_digest(&self.integrity)?.to_hex(),
        })
        .map_err(|e| corrupt(e.to_string()))
    }

    /// Checked restoration against independently reconstructed permanent event integrity.
    /// The adapter must first verify journal provenance and the configured authority key.
    pub fn restore_manifest(
        expected: &OutcomeKey,
        bytes: &[u8],
        integrity: TransactionReceipt,
    ) -> Result<Self, EventStoreError> {
        let wire: MarkerManifest =
            serde_json::from_slice(bytes).map_err(|e| corrupt(e.to_string()))?;
        integrity.validate_integrity()?;
        if wire.integrity != integrity_digest(&integrity)?.to_hex() {
            return Err(corrupt("manifest event-integrity digest mismatch"));
        }
        if wire.version != actor_format(wire.actor.as_ref())
            || wire.format != wire.version
            || wire.proposal_version != wire.version
            || wire.actor.as_ref() != integrity.actor()
            || wire.length < 4
            || wire.authority != expected.authority().as_str()
            || wire.context != expected.bounded_context().as_str()
            || wire.operation != expected.operation_id().as_str()
            || integrity.bounded_context() != Some(expected.bounded_context())
            || integrity.operation_id() != expected.operation_id()
            || integrity.security_scope().map(SecurityScope::as_str) != wire.scope.as_deref()
            || integrity.completion().is_some()
            || (wire.rejected && integrity.streams().iter().any(|s| !s.events().is_empty()))
        {
            return Err(corrupt(
                "manifest key, format, scope or integrity binding mismatch",
            ));
        }
        if let Some(evidence) = &wire.evidence {
            CommandCompletion::new(Some(evidence.clone()), None)
                .validate()
                .map_err(|e| corrupt(e.to_string()))?;
            if evidence.created_at_millis.is_some() {
                return Err(corrupt("retainable timestamp in permanent manifest"));
            }
        }
        let fingerprint =
            |value: &str| ContentFingerprint::from_hex(value).map_err(|e| corrupt(e.to_string()));
        let proposal =
            ProposalDigest::restore(wire.proposal_version, fingerprint(&wire.proposal)?)?;
        let reference = OutcomeDetailReference {
            key: expected.clone(),
            security_scope: integrity.security_scope().cloned(),
            actor: integrity.actor().cloned(),
            decision: if wire.rejected {
                OutcomeDecision::Rejected
            } else {
                OutcomeDecision::Accepted
            },
            format: wire.format,
            length: wire.length,
            digest: fingerprint(&wire.digest)?,
        };
        let revision = marker_revision(&reference, proposal, wire.evidence.as_ref(), &integrity)?;
        if revision.0 != fingerprint(&wire.revision)? {
            return Err(corrupt("manifest revision mismatch"));
        }
        Ok(Self {
            reference,
            revision,
            proposal,
            evidence: wire.evidence,
            integrity,
        })
    }
    /// Creates the marker/detail pair before the coordinator's atomic decision.
    /// This function alone does not confer commit authority.
    pub fn prepare(
        authority: CommandAuthority,
        proposal: &EventTransaction,
        receipt: TransactionReceipt,
    ) -> Result<(Self, Vec<u8>), EventStoreError> {
        receipt.validate_proposal(proposal)?;
        let context = proposal.bounded_context().ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "outcome requires context",
            )
        })?;
        let key = OutcomeKey::new(authority, context.clone(), proposal.operation_id().clone());
        let completion = proposal.completion().cloned();
        let bytes = EncodedOutcomeDetails::encode(
            completion.clone(),
            &key,
            proposal.security_scope(),
            proposal.actor(),
        )?;
        let reference = OutcomeDetailReference {
            key,
            security_scope: proposal.security_scope().cloned(),
            actor: proposal.actor().cloned(),
            decision: if completion
                .as_ref()
                .is_some_and(|value| value.rejection.is_some())
            {
                OutcomeDecision::Rejected
            } else {
                OutcomeDecision::Accepted
            },
            format: actor_format(proposal.actor()),
            length: bytes.len(),
            digest: ContentFingerprint::digest(&bytes),
        };
        let proposal = ProposalDigest::of(proposal)?;
        let evidence = completion
            .as_ref()
            .and_then(|v| v.evidence.clone())
            .map(|mut v| {
                // First timestamp is retainable detail, not command identity.
                v.created_at_millis = None;
                v
            });
        let integrity = receipt.without_completion();
        let revision = marker_revision(&reference, proposal, evidence.as_ref(), &integrity)?;
        Ok((
            Self {
                reference,
                revision,
                proposal,
                evidence,
                integrity,
            },
            bytes,
        ))
    }

    pub const fn detail_reference(&self) -> &OutcomeDetailReference {
        &self.reference
    }
    pub const fn actor(&self) -> Option<&crate::ActorRef> {
        self.reference.actor()
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

    /// Validates an adapter result against the exact requested storage coordinate.
    /// Authority strings identify storage; possession of one does not grant access.
    pub fn validate_key(&self, expected: &OutcomeKey) -> Result<(), EventStoreError> {
        if self.reference.key() != expected {
            return Err(corrupt(
                "completed outcome reference key differs from requested authority/context/operation",
            ));
        }
        Ok(())
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
            || self.actor() != metadata.actor()
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerManifest {
    version: u16,
    authority: String,
    context: String,
    operation: String,
    #[serde(deserialize_with = "required_optional")]
    scope: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_actor"
    )]
    actor: Option<crate::ActorRef>,
    rejected: bool,
    format: u16,
    length: usize,
    digest: String,
    proposal_version: u16,
    proposal: String,
    revision: String,
    integrity: String,
    #[serde(deserialize_with = "required_optional")]
    evidence: Option<OutcomeEvidence>,
}

fn required_optional<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(deserializer)
}

fn present_actor<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<crate::ActorRef>, D::Error> {
    crate::ActorRef::deserialize(deserializer).map(Some)
}

const fn actor_format(actor: Option<&crate::ActorRef>) -> u16 {
    if actor.is_some() { 2 } else { 1 }
}

fn integrity_digest(receipt: &TransactionReceipt) -> Result<ContentFingerprint, EventStoreError> {
    let streams: Vec<_> = receipt
        .streams()
        .iter()
        .map(|stream| {
            let events: Vec<_> = stream
                .events()
                .iter()
                .map(|event| {
                    let mut row = vec![
                        serde_json::json!(event.event_id().as_str()),
                        serde_json::json!(event.commit_id().as_str()),
                        serde_json::json!(event.stream_version().value()),
                        serde_json::json!(event.commit_event_ordinal()),
                        serde_json::json!(event.commit_event_count()),
                        serde_json::json!(event.event_type()),
                        serde_json::json!(event.schema_version()),
                        serde_json::json!(ContentFingerprint::digest(event.payload()).to_hex()),
                    ];
                    if receipt.actor().is_some() {
                        row.push(serde_json::json!(event.actor()));
                    }
                    serde_json::Value::Array(row)
                })
                .collect();
            serde_json::json!([
                stream.stream_id().aggregate_type().as_str(),
                stream.stream_id().aggregate_id().as_str(),
                stream.base_version().value(),
                events
            ])
        })
        .collect();
    let mut fields = vec![
        serde_json::json!(if receipt.actor().is_some() {
            "rostfrei:outcome-integrity:v2"
        } else {
            "rostfrei:outcome-integrity:v1"
        }),
        serde_json::json!(
            receipt
                .bounded_context()
                .map(rostfrei_messaging_core::BoundedContextName::as_str)
        ),
        serde_json::json!(receipt.operation_id().as_str()),
        serde_json::json!(receipt.operation_fingerprint().to_hex()),
        serde_json::json!(
            receipt
                .correlation_id()
                .map(rostfrei_messaging_core::CorrelationId::as_str)
        ),
        serde_json::json!(
            receipt
                .causation_id()
                .map(rostfrei_messaging_core::CausationId::as_str)
        ),
        serde_json::json!(receipt.security_scope().map(SecurityScope::as_str)),
    ];
    if receipt.actor().is_some() {
        fields.push(serde_json::json!(receipt.actor()));
    }
    fields.push(serde_json::json!(streams));
    let value = serde_json::Value::Array(fields);
    Ok(ContentFingerprint::digest(
        serde_json::to_vec(&value).map_err(|e| corrupt(e.to_string()))?,
    ))
}

fn marker_revision(
    reference: &OutcomeDetailReference,
    proposal: ProposalDigest,
    evidence: Option<&OutcomeEvidence>,
    integrity: &TransactionReceipt,
) -> Result<OutcomeRevision, EventStoreError> {
    let mut hash = Sha256::new();
    crate::identity::hash_part(
        &mut hash,
        if reference.actor().is_some() {
            b"rostfrei:outcome-revision:v3"
        } else {
            b"rostfrei:outcome-revision:v2"
        },
    );
    crate::identity::hash_part(&mut hash, &reference.encode()?);
    crate::identity::hash_part(&mut hash, &proposal.version().to_be_bytes());
    crate::identity::hash_part(&mut hash, proposal.fingerprint().as_bytes());
    crate::identity::hash_part(
        &mut hash,
        &serde_json::to_vec(&evidence).map_err(|e| corrupt(e.to_string()))?,
    );
    crate::identity::hash_part(&mut hash, integrity_digest(integrity)?.as_bytes());
    Ok(OutcomeRevision(ContentFingerprint::from_bytes(
        hash.finalize().into(),
    )))
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
    #[error("completed outcome details do not match the immutable winning reference")]
    IntegrityMismatch,
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
    pub const fn marker(&self) -> &OutcomeMarker {
        &self.marker
    }
    pub const fn details(&self) -> &OutcomeDetails<T> {
        &self.details
    }
}

impl<T: 'static> CompletedOutcome<T> {
    /// Checked assembly. Encoded details retain the exact reference validated at decode.
    pub fn new(
        marker: OutcomeMarker,
        details: OutcomeDetails<T>,
    ) -> Result<Self, OutcomeRecoveryError> {
        validate_detail_binding(&marker, &details)?;
        Ok(Self {
            marker: Box::new(marker),
            details,
        })
    }

    /// Checks both sides of a projection, including encoded values reintroduced by
    /// a mapping from an application DTO. Mapping never confers storage authority.
    pub fn try_map<U: 'static, E: From<OutcomeRecoveryError>>(
        self,
        map: impl FnOnce(T) -> Result<U, E>,
    ) -> Result<CompletedOutcome<U>, E> {
        validate_detail_binding(&self.marker, &self.details)?;
        let details = match self.details {
            OutcomeDetails::Available(value) => OutcomeDetails::Available(map(value)?),
            OutcomeDetails::Expired => OutcomeDetails::Expired,
            OutcomeDetails::Archived { reference } => OutcomeDetails::Archived { reference },
            OutcomeDetails::Unavailable { reason } => OutcomeDetails::Unavailable { reason },
        };
        CompletedOutcome::new(*self.marker, details).map_err(E::from)
    }
}

// Generic owned projections need no application-defined validation trait. Inspecting
// this one concrete type also prevents a projection from overriding encoded binding
// validation or laundering a foreign encoded value through an intermediate DTO.
fn validate_detail_binding<T: 'static>(
    marker: &OutcomeMarker,
    details: &OutcomeDetails<T>,
) -> Result<(), OutcomeRecoveryError> {
    if let OutcomeDetails::Available(value) = details {
        let value: &dyn Any = value;
        if let Some(encoded) = value.downcast_ref::<EncodedOutcomeDetails>()
            && encoded.reference != *marker.detail_reference()
        {
            return Err(OutcomeRecoveryError::IntegrityMismatch);
        }
    }
    Ok(())
}

impl CompletedOutcome<EncodedOutcomeDetails> {
    /// Consumer boundary for lookup, winner reconciliation and commit results.
    /// Invalid completed data never becomes Missing or authorizes reevaluation.
    pub fn validate_for(
        &self,
        key: &OutcomeKey,
        metadata: &CommandExecutionMetadata,
    ) -> Result<(), EventStoreError> {
        self.marker.validate_key(key)?;
        self.marker.validate_identity(metadata)?;
        self.marker.integrity_receipt().validate_integrity()?;
        validate_detail_binding(&self.marker, &self.details).map_err(|e| corrupt(e.to_string()))
    }
    /// Deliberate compatibility materialization. No inline receipt is stored by the split backend.
    pub fn to_inline(&self) -> Result<InlineCompletedOutcome, OutcomeRecoveryError> {
        validate_detail_binding(&self.marker, &self.details)?;
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
    reference: OutcomeDetailReference,
    completion: Option<CommandCompletion>,
}

impl EncodedOutcomeDetails {
    pub const fn detail_reference(&self) -> &OutcomeDetailReference {
        &self.reference
    }
    pub const fn completion(&self) -> Option<&CommandCompletion> {
        self.completion.as_ref()
    }
    pub fn rejection(&self) -> Option<&crate::EncodedRejection> {
        self.completion.as_ref().and_then(|v| v.rejection.as_ref())
    }

    fn encode(
        mut completion: Option<CommandCompletion>,
        key: &OutcomeKey,
        scope: Option<&SecurityScope>,
        actor: Option<&crate::ActorRef>,
    ) -> Result<Vec<u8>, EventStoreError> {
        let payload = completion
            .as_mut()
            .and_then(|v| v.rejection.as_mut())
            .map_or_else(Vec::new, |r| std::mem::take(&mut r.bytes));
        let header = DetailHeader {
            version: actor_format(actor),
            authority: key.authority().as_str().to_owned(),
            context: key.bounded_context().as_str().to_owned(),
            operation: key.operation_id().as_str().to_owned(),
            security_scope: scope.map(|v| v.as_str().to_owned()),
            actor: actor.cloned(),
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
        if reference.format != actor_format(reference.actor())
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
        if header.version != reference.format
            || header.actor.as_ref() != reference.actor()
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
            reference: reference.clone(),
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_actor"
    )]
    actor: Option<crate::ActorRef>,
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

#[cfg(test)]
mod binding_tests {
    use super::*;

    #[test]
    #[allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
    fn internal_pair_mixup_is_rechecked_before_materialization_or_mapping() {
        let authority = CommandAuthority::new("binding-test").unwrap();
        let prepare = |id: &str| {
            let operation = crate::OperationId::new(id).unwrap();
            let context = rostfrei_messaging_core::BoundedContextName::new("context").unwrap();
            let proposal =
                EventTransaction::new(operation.clone(), ContentFingerprint::digest(id), vec![])
                    .with_bounded_context(context.clone());
            let receipt =
                TransactionReceipt::new(operation, ContentFingerprint::digest(id), vec![])
                    .with_bounded_context(context);
            let (marker, bytes) =
                OutcomeMarker::prepare(authority.clone(), &proposal, receipt).unwrap();
            let details = EncodedOutcomeDetails::decode(marker.detail_reference(), &bytes).unwrap();
            (marker, details)
        };
        let (marker, _) = prepare("one");
        let (_, other) = prepare("two");
        // Deliberate private-field fault: public construction now prevents this pair.
        let invalid = CompletedOutcome {
            marker: Box::new(marker),
            details: OutcomeDetails::Available(other),
        };
        assert_eq!(
            invalid.to_inline().unwrap_err(),
            OutcomeRecoveryError::IntegrityMismatch
        );
        let mut called = false;
        let result = invalid.try_map::<_, OutcomeRecoveryError>(|_| {
            called = true;
            Ok(())
        });
        assert_eq!(result.unwrap_err(), OutcomeRecoveryError::IntegrityMismatch);
        assert!(!called);
    }
}
