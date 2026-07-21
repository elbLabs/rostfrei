use std::{error::Error as _, marker::PhantomData};

use async_nats::jetstream::{
    self,
    context::{PublishError, PublishErrorKind},
    kv::{self, Operation},
};
use async_trait::async_trait;
use rostfrei_core::{
    ReadModelCodec, ReadModelEntry, ReadModelError, ReadModelErrorKind, ReadModelKey,
    ReadModelRevision, ReadModelStore,
};

use crate::{
    NatsReadModelConfig,
    read_model_config::{unavailable, verify_config, verify_read_model},
};

/// Typed NATS KV storage sharing the application's managed `JetStream` context.
///
/// Reads use the stream leader. Mutations use per-key expected revisions and
/// wait for a persistence acknowledgement. A timeout is an uncertain outcome.
///
/// Handles own no background tasks or connections. Bucket deletion/recreation
/// requires reconnecting handles and discarding all old revision tokens.
pub struct NatsReadModelStore<T, C> {
    bucket: kv::Store,
    config: NatsReadModelConfig,
    codec: C,
    incarnation: String,
    marker: PhantomData<fn() -> T>,
}

impl<T, C: ReadModelCodec<T>> NatsReadModelStore<T, C> {
    /// Verification-only startup: this never creates or updates a bucket.
    pub async fn connect(
        context: jetstream::Context,
        config: NatsReadModelConfig,
        codec: C,
    ) -> Result<Self, ReadModelError> {
        // Verify even a non-KV stream occupying the expected name so malformed
        // bucket layout is a configuration mismatch, not an availability error.
        verify_read_model(&context, &config).await?;
        let bucket = context
            .get_key_value(config.bucket_name())
            .await
            .map_err(|_| unavailable())?;
        verify_config(&config, &bucket.stream.cached_info().config)?;
        let incarnation = format!(
            "{}:{}",
            config.bucket_name(),
            bucket.stream.cached_info().created.unix_timestamp_nanos()
        );
        Ok(Self {
            bucket,
            config,
            codec,
            incarnation,
            marker: PhantomData,
        })
    }

    fn revision(&self, key: &ReadModelKey, sequence: u64) -> ReadModelRevision {
        ReadModelRevision::from_token(format!("{}:{}:{sequence}", self.incarnation, key.as_str()))
    }

    fn sequence(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
    ) -> Result<u64, ReadModelError> {
        let prefix = format!("{}:{}:", self.incarnation, key.as_str());
        revision
            .as_token()
            .strip_prefix(&prefix)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                ReadModelError::new(
                    ReadModelErrorKind::InvalidRequest,
                    "revision belongs to another bucket, key, or incarnation",
                )
            })
    }

    fn encode(&self, value: &T) -> Result<Vec<u8>, ReadModelError> {
        let bytes = self.codec.encode(value)?;
        self.validate_size(&bytes)?;
        Ok(bytes)
    }

    fn validate_size(&self, bytes: &[u8]) -> Result<(), ReadModelError> {
        if i64::try_from(bytes.len())
            .map_or(true, |len| len > i64::from(self.config.max_value_bytes()))
        {
            return Err(ReadModelError::new(
                ReadModelErrorKind::PayloadTooLarge,
                "encoded read model exceeds configured value limit",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl<T, C> ReadModelStore<T> for NatsReadModelStore<T, C>
where
    T: Send + Sync,
    C: ReadModelCodec<T>,
{
    async fn read(&self, key: &ReadModelKey) -> Result<Option<ReadModelEntry<T>>, ReadModelError> {
        let entry = self
            .bucket
            .entry(key.as_str())
            .await
            .map_err(|_| unavailable())?;
        let Some(entry) = entry.filter(|entry| entry.operation == Operation::Put) else {
            return Ok(None);
        };
        self.validate_size(&entry.value)?;
        Ok(Some(ReadModelEntry {
            value: self.codec.decode(&entry.value)?,
            revision: self.revision(key, entry.revision),
        }))
    }

    async fn create(
        &self,
        key: &ReadModelKey,
        value: &T,
    ) -> Result<ReadModelRevision, ReadModelError> {
        let bytes = self.encode(value)?;
        // Read the tombstone revision explicitly. The client's create helper can
        // hide a publish failure as AlreadyExists; keep the original CAS error.
        let entry = self
            .bucket
            .entry(key.as_str())
            .await
            .map_err(|_| unavailable())?;
        let expected = match entry {
            Some(entry) if entry.operation == Operation::Put => return Err(conflict()),
            Some(entry) => entry.revision,
            None => 0,
        };
        let revision = self
            .bucket
            .update(key.as_str(), bytes.into(), expected)
            .await
            .map_err(|error| map_update_error(&error))?;
        Ok(self.revision(key, revision))
    }

    async fn update(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
        value: &T,
    ) -> Result<ReadModelRevision, ReadModelError> {
        let expected = self.sequence(key, revision)?;
        let bytes = self.encode(value)?;
        let revision = self
            .bucket
            .update(key.as_str(), bytes.into(), expected)
            .await
            .map_err(|error| map_update_error(&error))?;
        Ok(self.revision(key, revision))
    }

    async fn delete(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
    ) -> Result<(), ReadModelError> {
        self.bucket
            .delete_expect_revision(key.as_str(), Some(self.sequence(key, revision)?))
            .await
            .map_err(|error| map_update_error(&error))
    }
}

fn conflict() -> ReadModelError {
    ReadModelError::new(
        ReadModelErrorKind::Conflict,
        "read-model key or revision changed",
    )
}

fn map_update_error(error: &kv::UpdateError) -> ReadModelError {
    if error.kind() == kv::UpdateErrorKind::WrongLastRevision {
        return conflict();
    }
    let mut source = error.source();
    while let Some(cause) = source {
        if cause
            .downcast_ref::<PublishError>()
            .is_some_and(|error| error.kind() == PublishErrorKind::MaxPayloadExceeded)
        {
            return ReadModelError::new(
                ReadModelErrorKind::PayloadTooLarge,
                "read-model write exceeds server payload limit",
            );
        }
        if let Some(error) = cause.downcast_ref::<jetstream::Error>() {
            let kind = match error.error_code() {
                jetstream::ErrorCode::STREAM_MESSAGE_EXCEEDS_MAXIMUM => {
                    ReadModelErrorKind::PayloadTooLarge
                }
                jetstream::ErrorCode::STORAGE_RESOURCES_EXCEEDED
                | jetstream::ErrorCode::STREAM_LIMITS
                | jetstream::ErrorCode::STREAM_MAX_STREAM_BYTES_EXCEEDED => {
                    ReadModelErrorKind::CapacityExhausted
                }
                jetstream::ErrorCode::STREAM_STORE_FAILED
                    if error.to_string().starts_with("maximum bytes exceeded (") =>
                {
                    ReadModelErrorKind::CapacityExhausted
                }
                _ => ReadModelErrorKind::Unavailable,
            };
            return ReadModelError::new(kind, "read-model write rejected by storage");
        }
        source = cause.source();
    }
    unavailable()
}
