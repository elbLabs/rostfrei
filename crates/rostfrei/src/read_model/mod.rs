//! Typed, event-only materialization with framework-owned persistence and checkpoints.

mod builder;
mod integration;
mod runtime;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use rostfrei_core::{EventHistory, ReadModel, ReadModelError, ReadModelErrorKind, ReadModelStore};
use rostfrei_messaging_core::BoundedContext;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use builder::{NoDomainSource, ReadModelBuilder, ReadModels};
pub use integration::{IntegrationEventOrder, ReadModelIntegrationBinding};
pub use runtime::{ReadModelReader, ReadModelRuntime};

/// Persistence envelope owned by the runtime, not part of the query response.
/// Required metadata deliberately rejects old, uncheckpointed value-only records.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadModelState<M> {
    format_version: u32,
    value: M,
    positions: BTreeMap<String, u64>,
}

impl<M: Default> Default for ReadModelState<M> {
    fn default() -> Self {
        Self {
            format_version: 1,
            value: M::default(),
            positions: BTreeMap::new(),
        }
    }
}

impl<M> ReadModelState<M> {
    /// Read-only inspection of the application value for adapter implementations.
    pub const fn value(&self) -> &M {
        &self.value
    }

    fn validate(&self) -> Result<(), ReadModelError> {
        if self.format_version != 1
            || self
                .positions
                .iter()
                .any(|(source, position)| source.is_empty() || *position == 0)
        {
            return Err(ReadModelError::new(
                ReadModelErrorKind::InvalidData,
                "invalid read-model checkpoint envelope",
            ));
        }
        Ok(())
    }
}

/// Backend connection factory. Startup opens/verifies resources; provisioning is explicit.
#[async_trait]
pub trait ReadModelBackend: Send + Sync + 'static {
    async fn open<M: ReadModel>(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn ReadModelStore<ReadModelState<M>>>, ReadModelError>;

    /// Authoritative history used to fill gaps, including intentionally unhandled events.
    async fn history(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn EventHistory>, ReadModelError>;
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ReadModelProcessingError {
    #[error(transparent)]
    Storage(#[from] ReadModelError),
    #[error("invalid read-model registration: {0}")]
    Configuration(String),
    #[error("invalid source event: {0}")]
    InvalidEvent(String),
    #[error("source gap: expected {expected}, received {actual}")]
    SourceGap { expected: u64, actual: u64 },
    #[error("read-model transformation failed: {0}")]
    Transformation(String),
}

impl ReadModelProcessingError {
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Storage(error) if matches!(error.kind(), ReadModelErrorKind::Conflict | ReadModelErrorKind::Unavailable))
    }
}

type Change<M, E> = Arc<dyn Fn(&mut M, &E) -> Result<(), ReadModelProcessingError> + Send + Sync>;
type Key<E> = Arc<dyn Fn(&E) -> String + Send + Sync>;

/// Domain key-selection input. Dereferences to the typed payload, and exposes
/// committed metadata for events whose aggregate ID is not repeated in the payload.
pub struct ReadModelDomainEvent<'a, E> {
    pub(super) event: &'a E,
    pub(super) recorded: &'a rostfrei_core::RecordedEvent,
}

impl<E> ReadModelDomainEvent<'_, E> {
    pub const fn recorded(&self) -> &rostfrei_core::RecordedEvent {
        self.recorded
    }
}

impl<E> std::ops::Deref for ReadModelDomainEvent<'_, E> {
    type Target = E;
    fn deref(&self) -> &E {
        self.event
    }
}

type DomainKey<E> = Arc<dyn Fn(&ReadModelDomainEvent<'_, E>) -> String + Send + Sync>;
