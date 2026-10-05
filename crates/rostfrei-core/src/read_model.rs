//! Application-owned, latest-state storage. Source ordering and freshness belong
//! to the application; a storage revision is only a compare-and-set token.

use std::{marker::PhantomData, num::NonZeroU32};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Application-owned materialized query state. Derive through `rostfrei::ReadModel`.
/// The event runtime stores source checkpoints separately from these business fields.
pub trait ReadModel: Default + Serialize + DeserializeOwned + Send + Sync + 'static {
    const NAME: &'static str;
    const SCHEMA_VERSION: u32;
}

/// A concrete key, never a wildcard or a key enumeration request.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ReadModelKey(String);

impl ReadModelKey {
    /// Accepts 1–256 ASCII bytes: letters, digits, `-`, `_`, `/`, `=`, and
    /// dot-separated nonempty segments. Leading/trailing dots are invalid.
    pub fn new(value: impl Into<String>) -> Result<Self, ReadModelError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 256
            || value.split('.').any(str::is_empty)
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b'=' | b'.')
            })
        {
            return Err(ReadModelError::new(
                ReadModelErrorKind::InvalidRequest,
                "invalid read-model key",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An opaque, adapter-owned CAS token. It is neither an aggregate stream
/// version nor a source checkpoint. Use only with the store/key that issued it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadModelRevision(String);

impl ReadModelRevision {
    /// Adapter implementation hook. Applications should retain tokens returned
    /// by storage rather than construct or interpret them.
    pub fn from_token(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Adapter implementation hook; no ordering or numeric meaning is promised.
    pub fn as_token(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadModelEntry<T> {
    pub value: T,
    pub revision: ReadModelRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReadModelErrorKind {
    InvalidRequest,
    /// Create found a live value, or an update/delete revision did not match.
    Conflict,
    /// Includes uncertain write outcomes: reload before deciding to retry.
    Unavailable,
    InvalidData,
    IncompatibleSchema,
    EncodingFailed,
    PayloadTooLarge,
    CapacityExhausted,
    ConfigurationMismatch,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind:?}: {message}")]
pub struct ReadModelError {
    kind: ReadModelErrorKind,
    message: String,
}

impl ReadModelError {
    pub fn new(kind: ReadModelErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> ReadModelErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Typed storage for one application-owned read-model schema.
///
/// Each mutation is atomic for one key. There are no cross-key transactions,
/// source-ordering guarantees, or exactly-once handler semantics. Store source
/// checkpoints inside `T` so they commit with the materialized value.
#[async_trait]
pub trait ReadModelStore<T>: Send + Sync
where
    T: Send + Sync,
{
    /// Returns `None` for an absent, deleted, or expired key.
    async fn read(&self, key: &ReadModelKey) -> Result<Option<ReadModelEntry<T>>, ReadModelError>;

    /// Creates an absent key, including recreation after a tombstone/expiry.
    async fn create(
        &self,
        key: &ReadModelKey,
        value: &T,
    ) -> Result<ReadModelRevision, ReadModelError>;

    /// Replaces a value only at the supplied revision; never blindly upserts.
    async fn update(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
        value: &T,
    ) -> Result<ReadModelRevision, ReadModelError>;

    /// Writes a tombstone only at the supplied live revision. Repeating a
    /// successful delete with the old token conflicts. Re-read to resolve an
    /// uncertain acknowledgement. Recreation must use `create`.
    async fn delete(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
    ) -> Result<(), ReadModelError>;
}

/// Application-extensible serialization, including schema migration/validation.
///
/// Implementations should return `InvalidData` for malformed stored values and
/// `IncompatibleSchema` for deliberately unsupported versions.
pub trait ReadModelCodec<T>: Send + Sync {
    fn encode(&self, value: &T) -> Result<Vec<u8>, ReadModelError>;
    fn decode(&self, bytes: &[u8]) -> Result<T, ReadModelError>;
}

/// JSON envelope `{ "schema_version": N, "value": ... }`. Decoding accepts
/// exactly the configured nonzero version. Custom codecs can upcast old values.
#[derive(Clone, Debug)]
pub struct JsonReadModelCodec<T> {
    schema_version: NonZeroU32,
    marker: PhantomData<fn() -> T>,
}

impl<T> JsonReadModelCodec<T> {
    pub const fn new(schema_version: NonZeroU32) -> Self {
        Self {
            schema_version,
            marker: PhantomData,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonSnapshot<T> {
    schema_version: u32,
    value: T,
}

impl<T: Serialize + DeserializeOwned> ReadModelCodec<T> for JsonReadModelCodec<T> {
    fn encode(&self, value: &T) -> Result<Vec<u8>, ReadModelError> {
        serde_json::to_vec(&JsonSnapshot {
            schema_version: self.schema_version.get(),
            value,
        })
        .map_err(|_| {
            ReadModelError::new(
                ReadModelErrorKind::EncodingFailed,
                "read-model JSON encoding failed",
            )
        })
    }

    fn decode(&self, bytes: &[u8]) -> Result<T, ReadModelError> {
        let snapshot: JsonSnapshot<serde::de::IgnoredAny> =
            serde_json::from_slice(bytes).map_err(|_| {
                ReadModelError::new(
                    ReadModelErrorKind::InvalidData,
                    "invalid read-model JSON envelope",
                )
            })?;
        if snapshot.schema_version != self.schema_version.get() {
            return Err(ReadModelError::new(
                ReadModelErrorKind::IncompatibleSchema,
                "unsupported read-model schema version",
            ));
        }
        // Decode directly into T after checking the version. An intermediate
        // JSON Value would discard duplicate fields and limit integer fidelity.
        serde_json::from_slice::<JsonSnapshot<T>>(bytes)
            .map(|snapshot| snapshot.value)
            .map_err(|_| {
                ReadModelError::new(
                    ReadModelErrorKind::InvalidData,
                    "invalid read-model JSON value",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nonconcrete_or_unbounded_keys() {
        for key in [
            "",
            ".a",
            "a.",
            "a..b",
            "a.*",
            "a.>",
            "a b",
            "ümlaut",
            &"a".repeat(257),
        ] {
            assert_eq!(
                ReadModelKey::new(key).unwrap_err().kind(),
                ReadModelErrorKind::InvalidRequest
            );
        }
        assert!(ReadModelKey::new("org-1.member_2/summary=v1").is_ok());
    }

    #[test]
    fn checks_schema_before_decoding_the_application_value() {
        let codec = JsonReadModelCodec::<u64>::new(NonZeroU32::MIN);
        assert_eq!(codec.decode(&codec.encode(&42).unwrap()).unwrap(), 42);
        for bytes in [
            br"{}".as_slice(),
            br#"{"schema_version":1,"value":"bad"}"#,
            br"not json",
        ] {
            assert_eq!(
                codec.decode(bytes).unwrap_err().kind(),
                ReadModelErrorKind::InvalidData
            );
        }
        assert_eq!(
            codec
                .decode(br#"{"schema_version":2,"value":"different shape"}"#)
                .unwrap_err()
                .kind(),
            ReadModelErrorKind::IncompatibleSchema
        );
    }

    #[test]
    fn preserves_typed_validation_and_integer_fidelity() {
        #[derive(Debug, Deserialize, Serialize)]
        struct Snapshot {
            amount: u128,
        }
        let codec = JsonReadModelCodec::<Snapshot>::new(NonZeroU32::MIN);
        let bytes = codec.encode(&Snapshot { amount: u128::MAX }).unwrap();
        assert_eq!(codec.decode(&bytes).unwrap().amount, u128::MAX);
        assert_eq!(
            codec
                .decode(br#"{"schema_version":1,"value":{"amount":1,"amount":2}}"#)
                .unwrap_err()
                .kind(),
            ReadModelErrorKind::InvalidData
        );

        let codec =
            JsonReadModelCodec::<std::collections::BTreeMap<Vec<u8>, u8>>::new(NonZeroU32::MIN);
        assert_eq!(
            codec
                .encode(&std::collections::BTreeMap::from([(vec![1], 1)]))
                .unwrap_err()
                .kind(),
            ReadModelErrorKind::EncodingFailed
        );
    }
}
