#![allow(clippy::panic_in_result_fn)]

#[path = "../examples/read_model_storage/application.rs"]
mod application;
mod native_connection_support;
mod transaction_support;
use transaction_support::AppendOneTransaction;

#[path = "read_model/recovery.rs"]
mod recovery;

use std::{num::NonZeroU32, sync::Arc};

use application::{
    BicycleRegistered, ExampleResult, FleetEvent, FleetStatus, FleetStatusLookup,
    FleetStatusProjection, GetFleetStatus, InspectionChanged, RentalWindowChanged, rebuild,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, ExpectedVersion, NewEvent,
    OperationId, StreamId, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, DeliveryDisposition, DeliveryInfo, EnvelopeContext,
    IntegrationEventAddress, IntegrationEventEnvelope, MessageDelivery, MessageHandler, MessageId,
    MessageTimestamp, SchemaVersion,
};
use rostfrei_nats::{
    NatsEventStore, NatsEventStoreConfig, NatsReadModelConfig, NatsReadModelStore,
    provision_event_store, provision_read_model,
};
use rostfrei_runtime::{
    ApplicationName, DomainEventHandlerErrorKind, JsonReadModelCodec, QueryHandler,
    QueryHandlerRequest, ReadModelKey, ReadModelStore,
};

fn envelope_context() -> ExampleResult<EnvelopeContext> {
    Ok(EnvelopeContext::new(
        MessageId::new("workshop-7")?,
        SchemaVersion::new(1)?,
        CorrelationId::new("example")?,
        None,
    ))
}

fn workshop_delivery(
    context: &rostfrei_runtime::BoundedContext,
) -> ExampleResult<MessageDelivery<IntegrationEventAddress>> {
    let envelope = IntegrationEventEnvelope::new(
        envelope_context()?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        InspectionChanged {
            fleet_id: "fleet-1".to_owned(),
            source_version: 7,
            inspection_passed: true,
        },
    )?;
    Ok(MessageDelivery::new(
        context
            .application()
            .bounded_context("workshop")?
            .integration_event_address("inspection-changed")?,
        envelope.message_id().clone(),
        serde_json::to_vec(&envelope)?,
        CallerMetadata::default(),
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn multiple_handlers_merge_checkpoints_and_rebuild_from_authoritative_history()
-> ExampleResult {
    let client =
        native_connection_support::client(rostfrei_testing::integration::nats_url()?).await?;
    let jetstream = async_nats::jetstream::new(client);
    let context = ApplicationName::new(format!("read-model-projection-{}", std::process::id()))?
        .bounded_context("bike-rental")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(&jetstream, &events).await?;
    let history = NatsEventStore::connect(jetstream.clone(), events.clone()).await?;
    let config = NatsReadModelConfig::new(&context, "fleet-status")?;
    provision_read_model(&jetstream, &config).await?;
    let store = Arc::new(
        NatsReadModelStore::connect(
            jetstream.clone(),
            config.clone(),
            JsonReadModelCodec::<FleetStatus>::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let handlers = Arc::new(FleetStatusProjection::new(store.clone())?);
    let dispatcher = handlers.dispatcher()?;
    let stream = StreamId::new(AggregateType::new("fleet")?, AggregateId::new("fleet-1")?);
    let source_events = [
        FleetEvent::BicycleRegistered(BicycleRegistered),
        FleetEvent::RentalWindowChanged(RentalWindowChanged { open: true }),
    ];
    let operation = OperationId::new("projection-operation")?;
    let commit = derive_commit_id(&stream, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest("projection-source"),
        source_events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                Ok(NewEvent::new(
                    derive_event_id(&commit, u32::try_from(index)?),
                    event.event_type(),
                    1,
                    event.encode_json()?,
                )?)
            })
            .collect::<ExampleResult<Vec<_>>>()?,
    )?;
    let recorded = history
        .append_one_transaction(&stream, ExpectedVersion::NoStream, batch)
        .await?
        .into_events();
    let second = recorded.get(1).ok_or("missing event")?;
    assert_eq!(
        dispatcher.dispatch(second).await.unwrap_err().kind(),
        DomainEventHandlerErrorKind::OperatorBlocking
    );
    let key = ReadModelKey::new("fleet-1")?;
    assert!(store.read(&key).await?.is_none()); // A gap never advances the checkpoint.

    let (domain, integration) = tokio::join!(
        rebuild(&history, &stream, &dispatcher),
        MessageHandler::handle(handlers.as_ref(), workshop_delivery(&context)?)
    );
    domain?;
    assert_eq!(integration, DeliveryDisposition::Acknowledge);
    let expected = FleetStatus {
        bicycles_registered: 1,
        rentals_open: true,
        inspection_passed: true,
        fleet_version: 2,
        inspection_version: 7,
    };
    assert_eq!(handlers.lookup("fleet-1").await?, Some(expected.clone()));
    let before_duplicates = store.read(&key).await?.ok_or("missing snapshot")?.revision;
    rebuild(&history, &stream, &dispatcher).await?;
    assert_eq!(
        MessageHandler::handle(handlers.as_ref(), workshop_delivery(&context)?).await,
        DeliveryDisposition::Acknowledge
    );
    handlers
        .inspection_changed(&InspectionChanged {
            fleet_id: "fleet-1".to_owned(),
            source_version: 6,
            inspection_passed: false,
        })
        .await?;
    assert_eq!(
        store.read(&key).await?.ok_or("missing snapshot")?.revision,
        before_duplicates
    );

    let query = GetFleetStatus { lookup: handlers };
    let request = QueryHandlerRequest::new(
        envelope_context()?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        CallerMetadata::default(),
        None,
        "fleet-1".to_owned(),
    )?;
    assert_eq!(
        query
            .handle(request)
            .await
            .map_err(|error| error.message().to_owned())?,
        Some(expected.clone())
    );

    // Rebuild into a separately provisioned generation, leaving the serving
    // snapshot available until application-owned cutover.
    let next = NatsReadModelConfig::new(&context, "fleet-status-rebuild")?;
    provision_read_model(&jetstream, &next).await?;
    let next_store = Arc::new(
        NatsReadModelStore::connect(
            jetstream.clone(),
            next.clone(),
            JsonReadModelCodec::<FleetStatus>::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let rebuilt = Arc::new(FleetStatusProjection::new(next_store)?);
    rebuild(&history, &stream, &rebuilt.dispatcher()?).await?;
    assert!(
        !rebuilt
            .lookup("fleet-1")
            .await?
            .ok_or("missing rebuilt snapshot")?
            .inspection_passed
    );
    // Inspection is re-fetched from its authoritative owner; domain history alone
    // cannot recreate external-source facts.
    rebuilt
        .inspection_changed(&InspectionChanged {
            fleet_id: "fleet-1".to_owned(),
            source_version: 7,
            inspection_passed: true,
        })
        .await?;
    assert_eq!(rebuilt.lookup("fleet-1").await?, Some(expected));
    for stream in [
        config.stream_name(),
        next.stream_name(),
        events.stream_name().to_owned(),
    ] {
        jetstream.delete_stream(stream).await?;
    }
    Ok(())
}
