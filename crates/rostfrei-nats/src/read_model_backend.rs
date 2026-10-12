use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use async_nats::jetstream;
use async_trait::async_trait;
use rostfrei::{ReadModel, ReadModelBackend, ReadModelState};
use rostfrei_core::{
    EventHistory, JsonReadModelCodec, ReadModelError, ReadModelErrorKind, ReadModelStore,
};
use rostfrei_messaging_core::BoundedContext;

use crate::{NatsEventStore, NatsEventStoreConfig, NatsReadModelConfig, NatsReadModelStore};

/// Managed-connection backend for the event-only read-model runtime.
/// All opens verify existing resources; provision them explicitly during bootstrap.
pub struct NatsReadModelBackend {
    context: jetstream::Context,
    models: HashMap<String, NatsReadModelConfig>,
    history: Option<NatsEventStoreConfig>,
}

impl NatsReadModelBackend {
    pub fn new(context: jetstream::Context) -> Self {
        Self {
            context,
            models: HashMap::new(),
            history: None,
        }
    }

    /// Override the default bounded policy for a named model in this registry.
    #[must_use]
    pub fn with_model_config(mut self, config: NatsReadModelConfig) -> Self {
        self.models.insert(config.name().to_owned(), config);
        self
    }

    #[must_use]
    pub fn with_event_store_config(mut self, config: NatsEventStoreConfig) -> Self {
        self.history = Some(config);
        self
    }
}

#[async_trait]
impl ReadModelBackend for NatsReadModelBackend {
    async fn open<M: ReadModel>(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn ReadModelStore<ReadModelState<M>>>, ReadModelError> {
        let config = self
            .models
            .get(M::NAME)
            .cloned()
            .map_or_else(|| NatsReadModelConfig::for_model::<M>(context), Ok)?;
        if config.context() != context {
            return Err(mismatch("read-model policy belongs to another scope"));
        }
        let version = NonZeroU32::new(M::SCHEMA_VERSION)
            .ok_or_else(|| mismatch("read-model schema version must be positive"))?;
        Ok(Arc::new(
            NatsReadModelStore::connect(
                self.context.clone(),
                config,
                JsonReadModelCodec::new(version),
            )
            .await?,
        ))
    }

    async fn history(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn EventHistory>, ReadModelError> {
        let config = self
            .history
            .clone()
            .map_or_else(|| NatsEventStoreConfig::for_bounded_context(context), Ok)
            .map_err(|error| mismatch(&error.to_string()))?;
        let scope_matches = (
            config.application(),
            config.bounded_context(),
            config.traffic_scope(),
        ) == (
            context.application(),
            context.name(),
            context.traffic_scope(),
        );
        if !scope_matches {
            return Err(mismatch("read-model history belongs to another scope"));
        }
        let store = NatsEventStore::connect(self.context.clone(), config)
            .await
            .map_err(|error| {
                let kind = if error.kind() == rostfrei_core::EventStoreErrorKind::Unavailable {
                    ReadModelErrorKind::Unavailable
                } else {
                    ReadModelErrorKind::ConfigurationMismatch
                };
                ReadModelError::new(kind, error.to_string())
            })?;
        Ok(Arc::new(store))
    }
}

fn mismatch(message: &str) -> ReadModelError {
    ReadModelError::new(ReadModelErrorKind::ConfigurationMismatch, message)
}
