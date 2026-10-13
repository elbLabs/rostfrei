use crate::{
    CorrelationError, CorrelationObserver, DomainEventObservation, IntegrationEventObservation,
};
use async_trait::async_trait;
use rostfrei_messaging_core::{IntegrationEventAddress, IntegrationEventEnvelope};
use rostfrei_nats::{
    CorrelatedMessage, CorrelatedMessageFamily, CorrelatedMessageHandler, NatsEventStore,
};
use serde_json::Value;

pub(super) struct TracerCorrelationHandler {
    pub observer: CorrelationObserver,
    pub store: NatsEventStore,
    pub bindings: std::sync::Arc<crate::ApplicationBindings>,
}

#[async_trait]
impl CorrelatedMessageHandler for TracerCorrelationHandler {
    async fn handle(&self, message: CorrelatedMessage) {
        let result = match message.family() {
            CorrelatedMessageFamily::DomainEvent => match domain_observation(&self.store, &message)
            {
                Ok(event) => {
                    self.observer
                        .observe_domain_event(message.correlation_id().as_str(), event)
                        .await
                }
                Err(error) => Err(CorrelationError::InvalidId(error)),
            },
            CorrelatedMessageFamily::IntegrationEvent => {
                match integration_observation(&self.bindings, &message) {
                    Ok(event) => {
                        self.observer
                            .observe_integration_event(message.correlation_id().as_str(), event)
                            .await
                    }
                    Err(error) => Err(CorrelationError::InvalidId(error)),
                }
            }
        };
        if let Err(error) = result {
            let identity = message
                .message_id()
                .map_or_else(|| message.subject().to_owned(), |id| id.as_str().to_owned());
            let _ = self
                .observer
                .record_observation_failure(
                    message.correlation_id().as_str(),
                    identity,
                    error.to_string(),
                )
                .await;
        }
    }
}

fn domain_observation(
    store: &NatsEventStore,
    message: &CorrelatedMessage,
) -> Result<DomainEventObservation, String> {
    let event = store
        .decode_observed_event(message.subject(), message.headers(), message.payload())
        .map_err(|e| e.to_string())?;
    if event.correlation_id() != Some(message.correlation_id()) {
        return Err("stored event correlation does not match delivery header".into());
    }
    if message
        .message_id()
        .map(rostfrei_messaging_core::MessageId::as_str)
        != Some(event.event_id().as_str())
    {
        return Err("stored event identity does not match delivery header".into());
    }
    let payload: Value = serde_json::from_slice(event.payload()).map_err(|e| e.to_string())?;
    let mut observation = DomainEventObservation::new(
        event.event_id().as_str(),
        event.event_type(),
        event.schema_version(),
    )
    .with_aggregate(
        event.stream_id().aggregate_type().as_str(),
        event.stream_id().aggregate_id().as_str(),
    )
    .with_stream_version(event.stream_version().value())
    .with_payload(payload);
    if let Some(causation) = event.causation_id() {
        observation = observation.with_causation_id(causation.as_str());
    }
    Ok(observation)
}

fn integration_observation(
    bindings: &crate::ApplicationBindings,
    message: &CorrelatedMessage,
) -> Result<IntegrationEventObservation, String> {
    let address: IntegrationEventAddress = message
        .subject()
        .parse()
        .map_err(|e: rostfrei_messaging_core::ContractError| e.to_string())?;
    let envelope: IntegrationEventEnvelope<Value> =
        serde_json::from_slice(message.payload()).map_err(|e| e.to_string())?;
    let causation = envelope
        .causation_id()
        .ok_or("integration event has no causation identity")?;
    let source = rostfrei::EventId::new(causation.as_str()).map_err(|e| e.to_string())?;
    let expected = rostfrei::integration_message_id(&address, envelope.schema_version(), &source)
        .map_err(|e| e.to_string())?;
    if message.message_id() != Some(envelope.message_id())
        || envelope.message_id() != &expected
        || envelope.correlation_id() != message.correlation_id()
    {
        return Err(
            "integration-event envelope identity does not match delivery headers and provenance"
                .into(),
        );
    }
    let decoder = bindings
        .integration_decoders
        .get(&(address.name(), envelope.schema_version().get()))
        .copied()
        .ok_or("integration event codec is not registered")?;
    let encoded = rostfrei::EncodedIntegrationMessage::from_delivery(
        address,
        expected,
        message.payload().to_vec(),
        Some(message.correlation_id().clone()),
    )
    .map_err(|e| e.to_string())?;
    decoder(&encoded)
}
