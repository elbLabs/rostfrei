//! Run using a disposable NATS: `cargo xtask test -- cargo run
//! --locked -p rostfrei-nats --example read_model_storage`.

mod application;

use std::{num::NonZeroU32, sync::Arc};

use application::{
    BicycleRegistered, ExampleResult, FleetEvent, FleetStatus, FleetStatusProjection,
    GetFleetStatus, InspectionChanged, RentalWindowChanged, rebuild,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, EventStore,
    EventTransaction, ExpectedVersion, NewEvent, OperationId, StreamId, TransactionParticipant,
    derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, EnvelopeContext, MessageId, MessageTimestamp, SchemaVersion,
};
use rostfrei_nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, NatsReadModelConfig,
    NatsReadModelStore, connect, provision_event_store, provision_read_model,
};
use rostfrei_runtime::{
    ApplicationName, JsonReadModelCodec, QueryHandler, QueryHandlerRequest, ReadModelStore,
};

#[tokio::main]
async fn main() -> ExampleResult {
    let connection = connect(&NatsConnectionConfig::new(
        "read-model-example",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context = ApplicationName::new("read-model-example")?.bounded_context("bike-rental")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("read-model-storage-example")?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(connection.jetstream(), &events).await?;
    let history = NatsEventStore::connect(connection.jetstream().clone(), events).await?;
    let config = NatsReadModelConfig::new(&context, "fleet-status")?;
    provision_read_model(connection.jetstream(), &config).await?;
    let store: Arc<dyn ReadModelStore<FleetStatus>> = Arc::new(
        NatsReadModelStore::connect(
            connection.jetstream().clone(),
            config,
            JsonReadModelCodec::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let handlers = Arc::new(FleetStatusProjection::new(store)?);
    let dispatcher = handlers.dispatcher()?;
    let stream = StreamId::new(AggregateType::new("fleet")?, AggregateId::new("fleet-1")?);
    let source_events = [
        FleetEvent::BicycleRegistered(BicycleRegistered),
        FleetEvent::RentalWindowChanged(RentalWindowChanged { open: true }),
    ];
    let operation = OperationId::new("example-operation")?;
    let commit = derive_commit_id(&stream, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation.clone(),
        ContentFingerprint::digest("example-source"),
        source_events
            .iter()
            .enumerate()
            .map(|(index, event)| {
                Ok(NewEvent::new(
                    derive_event_id(&commit, u32::try_from(index)?),
                    event.event_type(),
                    event.schema_version(),
                    event.encode_json()?,
                )?)
            })
            .collect::<ExampleResult<Vec<_>>>()?,
    )?;
    history
        .append_transaction(
            EventTransaction::new(
                operation,
                batch.operation_fingerprint(),
                vec![TransactionParticipant::new(
                    stream.clone(),
                    ExpectedVersion::NoStream,
                    Some(batch),
                )],
            )
            .with_bounded_context(history.config().bounded_context().clone()),
        )
        .await?;
    rebuild(&history, &stream, &dispatcher).await?;
    rebuild(&history, &stream, &dispatcher).await?;
    handlers
        .inspection_changed(&InspectionChanged {
            fleet_id: "fleet-1".to_owned(),
            source_version: 7,
            inspection_passed: true,
        })
        .await?;
    let query = GetFleetStatus { lookup: handlers };
    let request = QueryHandlerRequest::new(
        EnvelopeContext::new(
            MessageId::new("query-1")?,
            SchemaVersion::new(1)?,
            CorrelationId::new("example")?,
            None,
        ),
        MessageTimestamp::from_unix_milliseconds(1)?,
        CallerMetadata::default(),
        None,
        "fleet-1".to_owned(),
    )?;
    let result = query
        .handle(request)
        .await
        .map_err(|error| error.message().to_owned())?;
    println!("{result:?}");
    Ok(())
}
