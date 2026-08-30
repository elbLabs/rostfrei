#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc};

use bike_rental::{
    nats_runtime::{BicycleRentalStarted, BicycleRentedIntegrationMapper, BikeRentalNatsConfig},
    rental::{BicycleId, BicycleRented, RentBicycle, RentalFleetAggregate},
    runtime::{demo_stream, seed_demo},
};
use rostfrei::{
    CommandBus, CommandMessageAdapter, CommandProcessor, CommandRequest, DomainEventDefinitionType,
    DomainEventDispatchOutcome, DomainEventDispatcher, DynamicCommandRequest, EventStore,
    InMemoryEventStore, InMemoryMessagingAdapter, IntegrationEventBus, IntegrationMessageAdapter,
    JsonDomainRejectionMapper, OperationId,
};
use rostfrei_messaging_core::{
    CausationId, CommandRejectionClassification, CommandResponseOutcome, CorrelationId,
};
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn processor(store: InMemoryEventStore) -> TestResult<CommandProcessor> {
    let store: Arc<dyn EventStore> = Arc::new(store);
    let mut processor = CommandProcessor::new(store);
    processor.register::<RentBicycle, _>(JsonDomainRejectionMapper::new(
        CommandRejectionClassification::Conflict,
    ))?;
    Ok(processor)
}

fn request(operation: &str, bicycle: &str) -> TestResult<CommandRequest<RentBicycle>> {
    Ok(CommandRequest::new(
        OperationId::new(operation)?,
        demo_stream()?.aggregate_id().clone(),
        RentBicycle {
            bicycle_id: BicycleId::new(bicycle).ok_or("invalid bicycle fixture")?,
        },
    ))
}

fn command_bus(
    config: &BikeRentalNatsConfig,
    adapter: Arc<InMemoryMessagingAdapter>,
) -> CommandBus {
    let adapter: Arc<dyn CommandMessageAdapter> = adapter;
    CommandBus::new(config.context().clone(), adapter)
}

#[test]
fn nats_configuration_uses_stable_application_scoped_resources() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;

    assert_eq!(
        config.command_address().as_str(),
        "bike-rental-demo.command.bike-rental.rent-bicycle"
    );
    assert_eq!(
        config.messaging().topology().command_stream().as_str(),
        "BIKE_RENTAL_DEMO_COMMANDS"
    );
    assert_eq!(
        config
            .messaging()
            .topology()
            .command_response_stream()
            .as_str(),
        "BIKE_RENTAL_DEMO_COMMAND_RESPONSES"
    );
    assert_eq!(
        config.event_store().stream_name(),
        "BIKE_RENTAL_DEMO__BIKE_RENTAL_DOMAIN_EVENTS"
    );
    assert_eq!(
        config.command_consumer().durable_name().as_str(),
        "bike-rental-demo--bike-rental--rent-bicycle--v1"
    );
    assert_eq!(
        config.integration_event_consumer().durable_name().as_str(),
        "bike-rental-demo--bike-rental--bicycle-rental-started-consumer--v1"
    );
    Ok(())
}

#[tokio::test]
async fn typed_command_bus_preserves_identity_replay_and_rejection_semantics() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;
    let first_adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor(
        store.clone(),
    )?)));
    let first_bus = command_bus(&config, Arc::clone(&first_adapter));
    let correlation = CorrelationId::new("rental-correlation")?;
    let causation = CausationId::new("incoming-rental-request")?;

    let first = first_bus
        .dispatch(
            request("rent-bike-42", "bike-42")?
                .with_correlation_id(correlation.clone())
                .with_causation_id(causation.clone()),
        )
        .await?;
    assert!(!first.publication_duplicate());
    assert!(matches!(
        first.response().outcome(),
        CommandResponseOutcome::Accepted
    ));

    let duplicate = first_bus
        .dispatch(
            request("rent-bike-42", "bike-42")?
                .with_correlation_id(correlation.clone())
                .with_causation_id(causation.clone()),
        )
        .await?;
    assert!(duplicate.publication_duplicate());
    assert_eq!(first.response(), duplicate.response());

    let encoded = first_adapter.command_messages().await;
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0].correlation_id(), &correlation);
    assert_eq!(
        encoded[0].message_id(),
        duplicate.response().command_message_id()
    );

    // A fresh transport adapter exercises event-store exact replay rather than transport deduplication.
    let replay_adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor(
        store.clone(),
    )?)));
    let replay_bus = command_bus(&config, replay_adapter);
    let replay = replay_bus
        .dispatch(
            request("rent-bike-42", "bike-42")?
                .with_correlation_id(correlation.clone())
                .with_causation_id(causation.clone()),
        )
        .await?;
    assert!(matches!(
        replay.response().outcome(),
        CommandResponseOutcome::Accepted
    ));

    let history = store.load(&demo_stream()?).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[1].correlation_id().map(CorrelationId::as_str),
        Some(correlation.as_str())
    );
    assert_eq!(
        history[1].causation_id().map(CausationId::as_str),
        Some(causation.as_str())
    );

    let rejected_adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor(
        store.clone(),
    )?)));
    let rejected_bus = command_bus(&config, rejected_adapter);
    let rejected = rejected_bus
        .dispatch(request("rent-bike-42-again", "bike-42")?)
        .await?;
    let CommandResponseOutcome::Rejected(rejection) = rejected.response().outcome() else {
        return Err("the second rental should be rejected".into());
    };
    assert_eq!(rejection.code().as_str(), "BICYCLE_UNAVAILABLE");
    assert_eq!(
        rejection
            .details()
            .and_then(|details| details.get("bicycle_id")),
        Some(&json!("bike-42"))
    );

    let conflict = rejected_bus
        .dispatch(request("rent-bike-42", "bike-99")?)
        .await?;
    let CommandResponseOutcome::Rejected(rejection) = conflict.response().outcome() else {
        return Err("operation identity reuse should be rejected".into());
    };
    assert_eq!(
        rejection.code().as_str(),
        "rostfrei.operation.identity-conflict"
    );
    assert_eq!(store.load(&demo_stream()?).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn dynamic_dispatch_rejects_unknown_commands_and_malformed_payloads() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor(store)?)));
    let bus = command_bus(&config, adapter);
    let aggregate_type = "bike-rental/rental-fleet";
    let aggregate_id = demo_stream()?.aggregate_id().clone();

    let unknown = bus
        .dispatch_dynamic(DynamicCommandRequest::new(
            OperationId::new("unknown-command")?,
            aggregate_type,
            aggregate_id.clone(),
            "missing-command",
            1,
            json!({}),
        )?)
        .await?;
    let CommandResponseOutcome::Rejected(rejection) = unknown.response().outcome() else {
        return Err("unknown command should be rejected".into());
    };
    assert_eq!(rejection.code().as_str(), "rostfrei.command.unknown");

    let malformed = bus
        .dispatch_dynamic(DynamicCommandRequest::new(
            OperationId::new("malformed-command")?,
            aggregate_type,
            aggregate_id,
            "rent-bicycle",
            1,
            json!({ "bicycle_id": "" }),
        )?)
        .await?;
    let CommandResponseOutcome::Rejected(rejection) = malformed.response().outcome() else {
        return Err("malformed payload should be rejected".into());
    };
    assert_eq!(
        rejection.code().as_str(),
        "rostfrei.command.invalid-payload"
    );
    Ok(())
}

#[tokio::test]
async fn post_commit_mapper_publishes_canonical_integration_event_once() -> TestResult {
    let config = BikeRentalNatsConfig::new("bike-rental-demo")?;
    let store = InMemoryEventStore::new();
    seed_demo(&store).await?;
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor(
        store.clone(),
    )?)));
    let bus = command_bus(&config, Arc::clone(&adapter));
    let command = bus
        .dispatch(
            request("integration-rental", "bike-42")?
                .with_correlation_id(CorrelationId::new("integration-correlation")?),
        )
        .await?;
    let history = store.load(&demo_stream()?).await?;
    let rented = history.get(1).ok_or("rental event was not committed")?;

    let integration_adapter: Arc<dyn IntegrationMessageAdapter> = adapter.clone();
    let integration_bus = IntegrationEventBus::new(config.context().clone(), integration_adapter);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register::<RentalFleetAggregate, BicycleRented, _>(
        BicycleRented::DEFINITION.id,
        Arc::new(BicycleRentedIntegrationMapper::new(integration_bus)),
    )?;

    assert_eq!(
        dispatcher.dispatch(rented).await?,
        DomainEventDispatchOutcome::Handled
    );
    assert_eq!(
        dispatcher.dispatch(rented).await?,
        DomainEventDispatchOutcome::Handled
    );
    let messages = adapter.integration_messages().await;
    assert_eq!(messages.len(), 1);
    let envelope = messages[0].decode::<BicycleRentalStarted>()?;
    assert_eq!(envelope.payload().fleet_id().as_str(), "city-fleet");
    assert_eq!(envelope.payload().bicycle_id().as_str(), "bike-42");
    assert_eq!(
        envelope.payload().source_event_id(),
        rented.event_id().as_str()
    );
    assert_eq!(
        envelope.correlation_id().as_str(),
        "integration-correlation"
    );
    assert_eq!(
        envelope.causation_id().map(CausationId::as_str),
        Some(rented.event_id().as_str())
    );
    assert_eq!(
        messages[0].address().as_str(),
        "bike-rental-demo.integration.bike-rental.bicycle-rental-started"
    );
    assert_eq!(
        command.response().correlation_id(),
        envelope.correlation_id()
    );
    Ok(())
}
