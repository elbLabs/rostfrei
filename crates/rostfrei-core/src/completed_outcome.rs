use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    EventCodecError, EventCodecErrorKind, EventStoreError, EventStoreErrorKind, EventTransaction,
    TransactionReceipt,
};

/// Independent of transport response TTL and publication deduplication. No identity reuse.
pub const MAX_REJECTION_BYTES: usize = 64 * 1024;

/// Auditable command identity dimensions in addition to the transaction's context,
/// operation, fingerprint, correlation and effective causation. Timestamp is audit-only.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeEvidence {
    pub authority_scope: Option<String>,
    pub created_at_millis: Option<u64>,
    pub fingerprint_version: u16,
    pub command_type: String,
    pub command_schema: u32,
    pub events_caused_by_command: bool,
    pub original_causation: Option<String>,
}

impl OutcomeEvidence {
    pub fn same_identity(&self, other: &Self) -> bool {
        self.authority_scope == other.authority_scope
            && self.fingerprint_version == other.fingerprint_version
            && self.command_type == other.command_type
            && self.command_schema == other.command_schema
            && self.events_caused_by_command == other.events_caused_by_command
            && self.original_causation == other.original_causation
    }
    pub fn validate(&self) -> Result<(), EventCodecError> {
        if self.fingerprint_version == 0
            || self.command_schema == 0
            || self.command_type.is_empty()
            || self.command_type.len() > 256
            || self.command_type.chars().any(char::is_control)
            || self
                .original_causation
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 256)
            || self.authority_scope.as_ref().is_some_and(|scope| {
                scope.is_empty() || scope.len() > 256 || scope.chars().any(char::is_control)
            })
        {
            return Err(codec_error(
                EventCodecErrorKind::InvalidEnvelope,
                "invalid completed command evidence",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncodedRejection {
    pub codec_id: String,
    pub codec_version: u32,
    pub bytes: Vec<u8>,
}

impl EncodedRejection {
    pub fn validate(&self) -> Result<(), EventCodecError> {
        if self.codec_id.is_empty()
            || self.codec_id.len() > 128
            || self.codec_id.chars().any(char::is_control)
            || self.codec_version == 0
            || self.bytes.len() > MAX_REJECTION_BYTES
        {
            return Err(codec_error(
                EventCodecErrorKind::InvalidEnvelope,
                "invalid or oversized encoded rejection",
            ));
        }
        Ok(())
    }
}

/// Versioned, protocol-neutral decision; never a domain event or a technical failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandCompletion {
    pub version: u16,
    pub evidence: Option<OutcomeEvidence>,
    pub rejection: Option<EncodedRejection>,
}

impl CommandCompletion {
    pub const fn new(
        evidence: Option<OutcomeEvidence>,
        rejection: Option<EncodedRejection>,
    ) -> Self {
        Self {
            version: 1,
            evidence,
            rejection,
        }
    }

    pub fn validate(&self) -> Result<(), EventCodecError> {
        if self.version != 1 {
            return Err(codec_error(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported completed outcome version",
            ));
        }
        if let Some(evidence) = &self.evidence {
            evidence.validate()?;
        }
        if let Some(rejection) = &self.rejection {
            rejection.validate()?;
        }
        Ok(())
    }
}

/// A single atomic winner, including its read guards and (only for acceptance) events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedOutcome(TransactionReceipt);

impl CompletedOutcome {
    pub const fn new(receipt: TransactionReceipt) -> Self {
        Self(receipt)
    }
    pub const fn record(&self) -> &TransactionReceipt {
        &self.0
    }
    pub fn acceptance(&self) -> Option<&TransactionReceipt> {
        self.rejection().is_none().then_some(&self.0)
    }
    pub fn rejection(&self) -> Option<&EncodedRejection> {
        self.0
            .completion()
            .and_then(|completion| completion.rejection.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletedAppendOutcome {
    Appended(CompletedOutcome),
    ExactReplay(CompletedOutcome),
}

impl CompletedAppendOutcome {
    pub const fn outcome(&self) -> &CompletedOutcome {
        match self {
            Self::Appended(outcome) | Self::ExactReplay(outcome) => outcome,
        }
    }
    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

/// Explicit bridge for arbitrary domain rejection types. Encoding happens before commit;
/// decoding failures block replay rather than re-running the handler.
pub trait RejectionCodec<R>: Send + Sync {
    fn encode(&self, rejection: &R) -> Result<EncodedRejection, EventCodecError>;
    fn decode(&self, rejection: &EncodedRejection) -> Result<R, EventCodecError>;
}

pub struct JsonRejectionCodec {
    id: String,
    version: u32,
}

impl JsonRejectionCodec {
    pub fn new(id: impl Into<String>, version: u32) -> Self {
        Self {
            id: id.into(),
            version,
        }
    }
}

impl<R: Serialize + DeserializeOwned> RejectionCodec<R> for JsonRejectionCodec {
    fn encode(&self, rejection: &R) -> Result<EncodedRejection, EventCodecError> {
        let encoded = EncodedRejection {
            codec_id: self.id.clone(),
            codec_version: self.version,
            bytes: serde_json::to_vec(rejection).map_err(|error| {
                codec_error(EventCodecErrorKind::EncodingFailed, error.to_string())
            })?,
        };
        encoded.validate()?;
        Ok(encoded)
    }
    fn decode(&self, rejection: &EncodedRejection) -> Result<R, EventCodecError> {
        rejection.validate()?;
        if rejection.codec_id != self.id || rejection.codec_version != self.version {
            return Err(codec_error(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported rejection codec identity/version",
            ));
        }
        serde_json::from_slice(&rejection.bytes)
            .map_err(|error| codec_error(EventCodecErrorKind::MalformedPayload, error.to_string()))
    }
}

pub(crate) struct UnconfiguredRejectionCodec;
impl<R> RejectionCodec<R> for UnconfiguredRejectionCodec {
    fn encode(&self, _: &R) -> Result<EncodedRejection, EventCodecError> {
        Err(codec_error(
            EventCodecErrorKind::EncodingFailed,
            "configure an explicit durable rejection codec with execute_with_rejection_codec",
        ))
    }
    fn decode(&self, _: &EncodedRejection) -> Result<R, EventCodecError> {
        Err(codec_error(
            EventCodecErrorKind::UnsupportedSchemaVersion,
            "no rejection replay codec configured",
        ))
    }
}

pub fn validate_completed_transaction(
    transaction: &EventTransaction,
) -> Result<(), EventStoreError> {
    if let Some(completion) = transaction.completion() {
        completion.validate().map_err(|error| {
            EventStoreError::new(EventStoreErrorKind::InvalidRequest, error.to_string())
        })?;
        if transaction.bounded_context().is_none()
            || (completion.rejection.is_some()
                && transaction
                    .participants()
                    .iter()
                    .any(|participant| participant.batch().is_some()))
        {
            return Err(EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "completed rejection requires context and read guards only",
            ));
        }
    }
    Ok(())
}

fn codec_error(kind: EventCodecErrorKind, message: impl Into<String>) -> EventCodecError {
    EventCodecError::new(kind, message)
}
