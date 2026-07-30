use std::{any::TypeId, sync::Arc, time::Duration};

use async_trait::async_trait;
use rostfrei_core::{ReadModel, ReadModelKey};
use rostfrei_messaging_core::{
    BoundedContext, DeliveryDisposition, IntegrationEventAddress, IntegrationEventEnvelope,
    MessageDelivery, MessageHandler, QuarantineReason, RetryDelay,
};

use super::{
    Change, Key, ReadModelProcessingError,
    runtime::{Mutation, Session},
};
use crate::IntegrationEvent;

#[derive(Clone, Copy)]
enum Ordering {
    Consecutive,
    Latest,
}

/// An explicit per-key integration source contract. The source name can be shared
/// by several event types from the same producer if they share one version sequence.
pub struct IntegrationEventOrder<E> {
    pub(super) source: String,
    ordering: Ordering,
    position: Arc<dyn Fn(&E) -> u64 + Send + Sync>,
}

impl<E> IntegrationEventOrder<E> {
    /// Deltas must be contiguous starting at 1. Missing versions block transformation.
    pub fn consecutive(
        source: impl Into<String>,
        position: impl Fn(&E) -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: source.into(),
            ordering: Ordering::Consecutive,
            position: Arc::new(position),
        }
    }

    /// A complete replacement fact supersedes all smaller versions. Use only when
    /// skipping intermediate source versions preserves the model's meaning.
    pub fn latest(
        source: impl Into<String>,
        position: impl Fn(&E) -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self {
            source: source.into(),
            ordering: Ordering::Latest,
            position: Arc::new(position),
        }
    }
}

pub(super) struct IntegrationSource<E> {
    pub context: BoundedContext,
    pub order: IntegrationEventOrder<E>,
}

/// Adapter-ready subscription. Only integration-event addresses can be represented.
pub struct ReadModelIntegrationBinding {
    pub(super) event_type: TypeId,
    pub(super) address: IntegrationEventAddress,
    pub(super) schema_version: u32,
    pub(super) handler: Arc<dyn MessageHandler<IntegrationEventAddress>>,
}

impl ReadModelIntegrationBinding {
    pub const fn address(&self) -> &IntegrationEventAddress {
        &self.address
    }
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn handler(&self) -> Arc<dyn MessageHandler<IntegrationEventAddress>> {
        self.handler.clone()
    }
    pub fn is_event<E: IntegrationEvent>(&self) -> bool {
        self.event_type == TypeId::of::<E>()
    }
}

pub(super) struct IntegrationHandler<M: ReadModel, E: IntegrationEvent> {
    session: Arc<Session<M>>,
    address: IntegrationEventAddress,
    source: String,
    order: IntegrationEventOrder<E>,
    key: Key<E>,
    change: Change<M, E>,
    retry_delay: RetryDelay,
    invalid: QuarantineReason,
    blocked: QuarantineReason,
}

impl<M: ReadModel, E: IntegrationEvent> IntegrationHandler<M, E> {
    pub fn bind(
        session: Arc<Session<M>>,
        input: IntegrationSource<E>,
        key: Key<E>,
        change: Change<M, E>,
    ) -> Result<ReadModelIntegrationBinding, ReadModelProcessingError> {
        let configuration = |message: String| ReadModelProcessingError::Configuration(message);
        let address = input
            .context
            .integration_event_address(E::EVENT_NAME)
            .map_err(|error| configuration(error.to_string()))?;
        let source = serde_json::to_string(&(
            "integration",
            input.context.name().as_str(),
            &input.order.source,
        ))
        .map_err(|error| configuration(error.to_string()))?;
        let handler = Self {
            session,
            source,
            address: address.clone(),
            order: input.order,
            key,
            change,
            retry_delay: RetryDelay::new(Duration::from_millis(100))
                .map_err(|error| configuration(error.to_string()))?,
            invalid: QuarantineReason::new("invalid read-model integration event")
                .map_err(|error| configuration(error.to_string()))?,
            blocked: QuarantineReason::new(
                "read model not materialized; repair the cause, then replay or refresh its source",
            )
            .map_err(|error| configuration(error.to_string()))?,
        };
        Ok(ReadModelIntegrationBinding {
            event_type: TypeId::of::<E>(),
            address,
            schema_version: E::SCHEMA_VERSION,
            handler: Arc::new(handler),
        })
    }

    async fn process(
        &self,
        delivery: &MessageDelivery<IntegrationEventAddress>,
    ) -> Result<(), ReadModelProcessingError> {
        let envelope: IntegrationEventEnvelope<E> = serde_json::from_slice(delivery.payload())
            .map_err(|_| {
                ReadModelProcessingError::InvalidEvent("malformed integration envelope".to_owned())
            })?;
        if delivery.address() != &self.address
            || envelope.schema_version().get() != E::SCHEMA_VERSION
            || envelope.message_id() != delivery.message_id()
        {
            return Err(ReadModelProcessingError::InvalidEvent(
                "integration address, schema or identity mismatch".to_owned(),
            ));
        }
        let event = envelope.payload();
        let key = ReadModelKey::new((self.key)(event))
            .map_err(|error| ReadModelProcessingError::InvalidEvent(error.to_string()))?;
        let version = (self.order.position)(event);
        self.session
            .mutate(
                &key,
                &self.source,
                version,
                &IntegrationMutation {
                    handler: self,
                    event,
                    version,
                },
            )
            .await
    }
}

struct IntegrationMutation<'a, M: ReadModel, E: IntegrationEvent> {
    handler: &'a IntegrationHandler<M, E>,
    event: &'a E,
    version: u64,
}

#[async_trait]
impl<M: ReadModel, E: IntegrationEvent> Mutation<M> for IntegrationMutation<'_, M, E> {
    async fn apply(&self, model: &mut M, applied: u64) -> Result<(), ReadModelProcessingError> {
        if matches!(self.handler.order.ordering, Ordering::Consecutive)
            && applied.checked_add(1) != Some(self.version)
        {
            return Err(ReadModelProcessingError::SourceGap {
                expected: applied.saturating_add(1),
                actual: self.version,
            });
        }
        (self.handler.change)(model, self.event)
    }
}

#[async_trait]
impl<M: ReadModel, E: IntegrationEvent> MessageHandler<IntegrationEventAddress>
    for IntegrationHandler<M, E>
{
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        match self.process(&delivery).await {
            Ok(()) => DeliveryDisposition::Acknowledge,
            Err(error) if error.is_retryable() => DeliveryDisposition::RetryAfter(self.retry_delay),
            Err(ReadModelProcessingError::InvalidEvent(_)) => {
                DeliveryDisposition::Quarantine(self.invalid.clone())
            }
            // Explicit quarantine and exhausted retries advance delivery progress
            // without materializing. Application-owned recovery must redrive it.
            Err(_) => DeliveryDisposition::Quarantine(self.blocked.clone()),
        }
    }
}
