use crate::{NatsReadModelConfig, NatsReadModelStore};
use async_trait::async_trait;
use rostfrei_core::{
    EventStoreError, EventStoreErrorKind, JsonReadModelCodec, ReadModelError, ReadModelErrorKind,
    ReadModelKey, ReadModelStore, SnapshotKey, SnapshotStore, StoredSnapshot, StreamVersion,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Catalog {
    versions: Vec<u64>,
}

/// Durable aggregate checkpoints in an explicitly provisioned, scoped KV bucket.
/// Retains up to 64 versions per aggregate/replay revision. Events are never deleted.
pub struct NatsSnapshotStore {
    catalogs: NatsReadModelStore<Catalog, JsonReadModelCodec<Catalog>>,
    values: NatsReadModelStore<StoredSnapshot, JsonReadModelCodec<StoredSnapshot>>,
}
impl NatsSnapshotStore {
    pub async fn connect(
        context: async_nats::jetstream::Context,
        config: NatsReadModelConfig,
    ) -> Result<Self, ReadModelError> {
        Ok(Self {
            catalogs: NatsReadModelStore::connect(
                context.clone(),
                config.clone(),
                JsonReadModelCodec::new(NonZeroU32::MIN),
            )
            .await?,
            values: NatsReadModelStore::connect(
                context,
                config,
                JsonReadModelCodec::new(NonZeroU32::MIN),
            )
            .await?,
        })
    }
}
#[allow(clippy::needless_pass_by_value, reason = "owned map_err adapter")]
fn map_error(error: ReadModelError) -> EventStoreError {
    let kind = match error.kind() {
        ReadModelErrorKind::Conflict => EventStoreErrorKind::Conflict,
        ReadModelErrorKind::Unavailable => EventStoreErrorKind::Unavailable,
        ReadModelErrorKind::CapacityExhausted => EventStoreErrorKind::CapacityExhausted,
        ReadModelErrorKind::ConfigurationMismatch => EventStoreErrorKind::ConfigurationMismatch,
        _ => EventStoreErrorKind::InvalidRequest,
    };
    EventStoreError::new(kind, error.to_string())
}
fn value_key(key: &SnapshotKey, version: u64) -> Result<ReadModelKey, EventStoreError> {
    ReadModelKey::new(format!("snapshot.{}.v{version}", key.as_str())).map_err(map_error)
}
#[async_trait]
impl SnapshotStore for NatsSnapshotStore {
    async fn load(
        &self,
        key: &SnapshotKey,
        at_or_before: StreamVersion,
    ) -> Result<Option<StoredSnapshot>, EventStoreError> {
        let key_catalog =
            ReadModelKey::new(format!("catalog.{}", key.as_str())).map_err(map_error)?;
        let Some(catalog) = self.catalogs.read(&key_catalog).await.map_err(map_error)? else {
            return Ok(None);
        };
        let Some(version) = catalog
            .value
            .versions
            .iter()
            .copied()
            .filter(|version| *version <= at_or_before.value())
            .max()
        else {
            return Ok(None);
        };
        Ok(self
            .values
            .read(&value_key(key, version)?)
            .await
            .map_err(map_error)?
            .map(|entry| entry.value))
    }
    async fn save(
        &self,
        key: &SnapshotKey,
        snapshot: &StoredSnapshot,
    ) -> Result<(), EventStoreError> {
        snapshot.validate()?;
        let version = snapshot.checkpoint.version;
        let value_key = value_key(key, version)?;
        match self.values.create(&value_key, snapshot).await {
            Ok(_) => {}
            Err(error) => {
                let existing = self.values.read(&value_key).await.map_err(map_error)?;
                if existing.is_none_or(|entry| {
                    entry.value.validate().is_err() || !entry.value.same_state(snapshot)
                }) {
                    return Err(map_error(error));
                }
            }
        }
        let key_catalog =
            ReadModelKey::new(format!("catalog.{}", key.as_str())).map_err(map_error)?;
        for _ in 0..16 {
            let existing = self.catalogs.read(&key_catalog).await.map_err(map_error)?;
            let mut versions = existing
                .as_ref()
                .map_or_else(Vec::new, |entry| entry.value.versions.clone());
            if versions.contains(&version) {
                return Ok(());
            }
            versions.push(version);
            versions.sort_unstable();
            let remove = versions.len().saturating_sub(64);
            let removed: Vec<_> = versions.drain(..remove).collect();
            let value = Catalog { versions };
            let write = match existing {
                Some(entry) => {
                    self.catalogs
                        .update(&key_catalog, &entry.revision, &value)
                        .await
                }
                None => self.catalogs.create(&key_catalog, &value).await,
            };
            match write {
                Ok(_) => {
                    for version in removed {
                        let old_key = self::value_key(key, version)?;
                        if let Ok(Some(old)) = self.values.read(&old_key).await {
                            let _ = self.values.delete(&old_key, &old.revision).await;
                        }
                    }
                    return Ok(());
                }
                Err(error) if error.kind() == ReadModelErrorKind::Conflict => {}
                Err(error) => return Err(map_error(error)),
            }
        }
        Err(EventStoreError::new(
            EventStoreErrorKind::Conflict,
            "snapshot catalog contention budget exhausted",
        ))
    }
}
