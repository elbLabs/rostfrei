use std::marker::PhantomData;

use async_trait::async_trait;
use rostfrei_core::{Aggregate, AggregateId, CommandHandler, OperationId};
use rostfrei_messaging_core::{
    CausationId, DeliveryDisposition, DurableName, IntegrationEventAddress,
    IntegrationEventEnvelope, MessageDelivery, MessageHandler, QuarantineReason, RetryDelay,
};
use rostfrei_registry::CommandDefinition;
use thiserror::Error;

use crate::{
    CommandBus, CommandBusError, CommandBusReceipt, CommandRequest, JsonCommandPayload,
    command_bus::framed_fingerprint,
    integration_event_bus::{EncodedIntegrationMessage, IntegrationEvent},
};

/// Maps one incoming integration event to one command for a target aggregate.
pub trait IntegrationCommandMapper<E>: Send + Sync {
    type Aggregate: Aggregate + CommandHandler<Self::Command>;
    type Command: CommandDefinition<Self::Aggregate> + JsonCommandPayload + Send + Sync;
    type Error;

    fn map(&self, event: &E) -> Result<IntegrationCommand<Self::Command>, Self::Error>;
}

/// A typed command and the aggregate instance that should receive it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationCommand<C> {
    aggregate_id: AggregateId,
    command: C,
}

impl<C> IntegrationCommand<C> {
    pub const fn new(aggregate_id: AggregateId, command: C) -> Self {
        Self {
            aggregate_id,
            command,
        }
    }

    pub const fn aggregate_id(&self) -> &AggregateId {
        &self.aggregate_id
    }

    pub const fn command(&self) -> &C {
        &self.command
    }

    pub fn into_parts(self) -> (AggregateId, C) {
        (self.aggregate_id, self.command)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedIntegrationCommand {
    receipt: CommandBusReceipt,
}

impl CompletedIntegrationCommand {
    pub const fn command_message_id(&self) -> &rostfrei_messaging_core::MessageId {
        self.receipt.response().command_message_id()
    }

    pub const fn publication_duplicate(&self) -> bool {
        self.receipt.publication_duplicate()
    }

    pub const fn response(&self) -> &rostfrei_messaging_core::CommandResponse {
        self.receipt.response()
    }

    pub fn into_response(self) -> rostfrei_messaging_core::CommandResponse {
        self.receipt.into_response()
    }
}

#[derive(Debug, Error)]
pub enum IntegrationEventProcessingError<MapperError> {
    #[error("integration command mapper failed")]
    Mapper(MapperError),
    #[error("deterministic operation identity could not be built: {0}")]
    MessageIdentity(String),
    #[error("command dispatch failed: {0}")]
    CommandBus(CommandBusError),
}

/// Dispatches the command produced by an incoming integration-event mapper.
pub struct IntegrationEventProcessor<Mapper> {
    command_bus: CommandBus,
    durable_name: DurableName,
    mapper: Mapper,
}

impl<Mapper> IntegrationEventProcessor<Mapper> {
    pub const fn new(command_bus: CommandBus, durable_name: DurableName, mapper: Mapper) -> Self {
        Self {
            command_bus,
            durable_name,
            mapper,
        }
    }

    pub async fn process<E>(
        &self,
        envelope: &IntegrationEventEnvelope<E>,
    ) -> Result<CompletedIntegrationCommand, IntegrationEventProcessingError<Mapper::Error>>
    where
        Mapper: IntegrationCommandMapper<E>,
        E: Sync,
    {
        let mapped = self
            .mapper
            .map(envelope.payload())
            .map_err(IntegrationEventProcessingError::Mapper)?;
        let (aggregate_id, command) = mapped.into_parts();
        let aggregate_type = <Mapper::Aggregate as Aggregate>::aggregate_type();
        let operation_id = integration_operation_id(
            &self.durable_name,
            envelope.message_id(),
            aggregate_type.as_ref(),
            aggregate_id.as_str(),
        )
        .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let causation_id = CausationId::new(envelope.message_id().as_str())
            .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let request = CommandRequest::new(operation_id, aggregate_id, command)
            .with_correlation_id(envelope.correlation_id().clone())
            .with_causation_id(causation_id)
            .with_created_at(envelope.occurred_at());
        let receipt = self
            .command_bus
            .dispatch::<Mapper::Aggregate, Mapper::Command>(request)
            .await
            .map_err(IntegrationEventProcessingError::CommandBus)?;
        Ok(CompletedIntegrationCommand { receipt })
    }
}

/// Adapts a typed integration-command mapping to the transport consumer port.
pub struct IntegrationEventCommandHandler<E, Mapper> {
    processor: IntegrationEventProcessor<Mapper>,
    retry_delay: RetryDelay,
    marker: PhantomData<fn() -> E>,
}

impl<E, Mapper> IntegrationEventCommandHandler<E, Mapper> {
    pub const fn new(
        command_bus: CommandBus,
        durable_name: DurableName,
        retry_delay: RetryDelay,
        mapper: Mapper,
    ) -> Self {
        Self {
            processor: IntegrationEventProcessor::new(command_bus, durable_name, mapper),
            retry_delay,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<E, Mapper> MessageHandler<IntegrationEventAddress>
    for IntegrationEventCommandHandler<E, Mapper>
where
    E: IntegrationEvent,
    Mapper: IntegrationCommandMapper<E>,
    Mapper::Error: Send,
{
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let envelope = EncodedIntegrationMessage::from_delivery(
            delivery.address().clone(),
            delivery.message_id().clone(),
            delivery.payload().to_vec(),
            delivery.correlation_id().cloned(),
        )
        .and_then(|message| message.decode::<E>());
        let Ok(envelope) = envelope else {
            return quarantine("invalid integration event envelope");
        };

        match self.processor.process(&envelope).await {
            Ok(_) => DeliveryDisposition::Acknowledge,
            Err(IntegrationEventProcessingError::CommandBus(error))
                if matches!(
                    error.kind(),
                    crate::CommandBusErrorKind::Timeout | crate::CommandBusErrorKind::Unavailable
                ) =>
            {
                DeliveryDisposition::RetryAfter(self.retry_delay)
            }
            Err(IntegrationEventProcessingError::Mapper(_)) => {
                quarantine("integration command mapping failed")
            }
            Err(_) => quarantine("integration command mapping produced an invalid command"),
        }
    }
}

fn quarantine(reason: &'static str) -> DeliveryDisposition {
    QuarantineReason::new(reason).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::Quarantine,
    )
}

fn integration_operation_id(
    durable_name: &DurableName,
    source_message_id: &rostfrei_messaging_core::MessageId,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<OperationId, rostfrei_core::IdentityError> {
    let fingerprint = framed_fingerprint(&[
        b"rostfrei:integration-operation:v1",
        durable_name.as_str().as_bytes(),
        source_message_id.as_str().as_bytes(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
    ]);
    OperationId::new(format!("integration:{}", fingerprint.to_hex()))
}
