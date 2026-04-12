use std::fmt::{self, Write};
use std::str::FromStr;

use rostfrei_messaging_core::{CausationId, CorrelationId};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_AGGREGATE_TYPE_LEN: usize = 128;
const MAX_AGGREGATE_ID_LEN: usize = 256;
const MAX_ID_LEN: usize = 128;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum IdentityError {
    #[error("{kind} must not be empty")]
    Empty { kind: &'static str },
    #[error("{kind} exceeds its {maximum}-byte limit")]
    TooLong { kind: &'static str, maximum: usize },
    #[error("{kind} must not have leading or trailing whitespace")]
    SurroundingWhitespace { kind: &'static str },
    #[error("{kind} must not contain control characters")]
    ControlCharacter { kind: &'static str },
    #[error("content fingerprint must be exactly 64 hexadecimal characters")]
    InvalidFingerprint,
}

fn validate(value: &str, kind: &'static str, maximum: usize) -> Result<(), IdentityError> {
    if value.is_empty() {
        return Err(IdentityError::Empty { kind });
    }
    if value.len() > maximum {
        return Err(IdentityError::TooLong { kind, maximum });
    }
    if value.trim() != value {
        return Err(IdentityError::SurroundingWhitespace { kind });
    }
    if value.chars().any(char::is_control) {
        return Err(IdentityError::ControlCharacter { kind });
    }
    Ok(())
}

macro_rules! string_identity {
    ($name:ident, $kind:literal, $maximum:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                validate(&value, $kind, $maximum)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

string_identity!(AggregateType, "aggregate type", MAX_AGGREGATE_TYPE_LEN);
string_identity!(AggregateId, "aggregate id", MAX_AGGREGATE_ID_LEN);
string_identity!(EventId, "event id", MAX_ID_LEN);
string_identity!(CommitId, "commit id", MAX_ID_LEN);
string_identity!(OperationId, "operation id", MAX_ID_LEN);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StreamId {
    aggregate_type: AggregateType,
    aggregate_id: AggregateId,
}

impl StreamId {
    pub fn new(aggregate_type: AggregateType, aggregate_id: AggregateId) -> Self {
        Self {
            aggregate_type,
            aggregate_id,
        }
    }

    pub fn aggregate_type(&self) -> &AggregateType {
        &self.aggregate_type
    }

    pub fn aggregate_id(&self) -> &AggregateId {
        &self.aggregate_id
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.aggregate_type, self.aggregate_id)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ContentFingerprint([u8; 32]);

impl ContentFingerprint {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn digest(content: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(content.as_ref()).into())
    }

    pub fn from_hex(value: &str) -> Result<Self, IdentityError> {
        if value.len() != 64 {
            return Err(IdentityError::InvalidFingerprint);
        }

        let mut bytes = [0_u8; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = decode_hex(pair[0]).ok_or(IdentityError::InvalidFingerprint)?;
            let low = decode_hex(pair[1]).ok_or(IdentityError::InvalidFingerprint)?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        let mut encoded = String::with_capacity(64);
        for byte in self.0 {
            write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
        }
        encoded
    }
}

impl fmt::Display for ContentFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ContentFingerprint {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_hex(value)
    }
}

const fn decode_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionMetadata {
    stream_id: StreamId,
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    commit_id: CommitId,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
}

impl ExecutionMetadata {
    pub fn new(
        stream_id: StreamId,
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
    ) -> Self {
        let commit_id = derive_commit_id(&stream_id, &operation_id);
        Self {
            stream_id,
            operation_id,
            operation_fingerprint,
            commit_id,
            correlation_id: None,
            causation_id: None,
        }
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
    }

    pub fn commit_id(&self) -> &CommitId {
        &self.commit_id
    }

    pub const fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub fn event_id(&self, ordinal: u32) -> EventId {
        derive_event_id(&self.commit_id, ordinal)
    }
}

pub(crate) fn derive_commit_id(stream_id: &StreamId, operation_id: &OperationId) -> CommitId {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, b"rostfrei:commit:v1");
    hash_part(&mut hasher, stream_id.aggregate_type().as_str().as_bytes());
    hash_part(&mut hasher, stream_id.aggregate_id().as_str().as_bytes());
    hash_part(&mut hasher, operation_id.as_str().as_bytes());
    CommitId::new(format!("commit:{:x}", hasher.finalize()))
        .expect("derived commit identities are always valid")
}

pub(crate) fn derive_event_id(commit_id: &CommitId, ordinal: u32) -> EventId {
    let mut hasher = Sha256::new();
    hash_part(&mut hasher, b"rostfrei:event:v1");
    hash_part(&mut hasher, commit_id.as_str().as_bytes());
    hash_part(&mut hasher, &ordinal.to_be_bytes());
    EventId::new(format!("event:{:x}", hasher.finalize()))
        .expect("derived event identities are always valid")
}

fn hash_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update((part.len() as u64).to_be_bytes());
    hasher.update(part);
}
