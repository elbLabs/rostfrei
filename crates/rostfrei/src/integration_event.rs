use std::marker::PhantomData;

use async_trait::async_trait;
use rostfrei_core::OperationId;
use rostfrei_messaging_core::{
    CausationId, DeliveryDisposition, DurableName, IntegrationEventAddress,
    IntegrationEventEnvelope, MessageDelivery, MessageHandler, QuarantineReason, RetryDelay,
};

use thiserror::Error;

use crate::{
    CommandBus, CommandBusError, CommandBusReceipt, CommandRequest, JsonCommandPayload,
    command_bus::framed_fingerprint,
    integration_event_bus::{EncodedIntegrationMessage, IntegrationEvent},
};

/// Maps one incoming integration event to one self-contained command.
pub trait IntegrationCommandMapper<E>: Send + Sync {
    type Command: JsonCommandPayload + Send + Sync;
    type Error;

    fn map(&self, event: &E) -> Result<Self::Command, Self::Error>;
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
        let command = self
            .mapper
            .map(envelope.payload())
            .map_err(IntegrationEventProcessingError::Mapper)?;
        let operation_id = integration_operation_id(&self.durable_name, envelope.message_id())
            .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let causation_id = CausationId::new(envelope.message_id().as_str())
            .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let request = CommandRequest::new(operation_id, command)
            .with_correlation_id(envelope.correlation_id().clone())
            .with_causation_id(causation_id)
            .with_created_at(envelope.occurred_at())
            .with_events_caused_by_command();
        let receipt = self
            .command_bus
            .dispatch::<Mapper::Command>(request)
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
) -> Result<OperationId, rostfrei_core::IdentityError> {
    let fingerprint = framed_fingerprint(&[
        b"rostfrei:integration-operation:v2",
        durable_name.as_str().as_bytes(),
        source_message_id.as_str().as_bytes(),
    ]);
    OperationId::new(format!("integration:{}", fingerprint.to_hex()))
}
