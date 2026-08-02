//! Run using a disposable NATS: `python3 scripts/test_nats.py -- cargo run
//! --locked -p rostfrei-nats --example read_model_storage`.

mod application;

use std::{num::NonZeroU32, sync::Arc};

use application::{
    BillingChanged, DemoChanged, Entitlement, Entitlements, ExampleResult, GetEntitlement,
    MemberJoined, OrganizationEvent, rebuild,
};
use rostfrei::{
    ApplicationName, JsonReadModelCodec, QueryHandler, QueryHandlerRequest, ReadModelStore,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, StreamId, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, EnvelopeContext, MessageId, MessageTimestamp, SchemaVersion,
};
use rostfrei_nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, NatsReadModelConfig,
    NatsReadModelStore, connect, provision_event_store, provision_read_model,
};

#[tokio::main]
async fn main() -> ExampleResult {
    let connection = connect(&NatsConnectionConfig::new(
        "read-model-example",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context = ApplicationName::new("read-model-example")?.bounded_context("access")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(connection.jetstream(), &events).await?;
    let history = NatsEventStore::connect(connection.jetstream().clone(), events).await?;
    let config = NatsReadModelConfig::new(&context, "entitlements")?;
    provision_read_model(connection.jetstream(), &config).await?;
    let store: Arc<dyn ReadModelStore<Entitlement>> = Arc::new(
        NatsReadModelStore::connect(
            connection.jetstream().clone(),
            config,
            JsonReadModelCodec::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let handlers = Arc::new(Entitlements::new(store)?);
    let dispatcher = handlers.dispatcher()?;
    let stream = StreamId::new(
        AggregateType::new("organization")?,
        AggregateId::new("org-1")?,
    );
    let source_events = [
        OrganizationEvent::MemberJoined(MemberJoined),
        OrganizationEvent::DemoChanged(DemoChanged { active: true }),
    ];
    let operation = OperationId::new("example-operation")?;
    let commit = derive_commit_id(&stream, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation,
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
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await?;
    rebuild(&history, &stream, &dispatcher).await?;
    rebuild(&history, &stream, &dispatcher).await?;
    handlers
        .billing_changed(&BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: 7,
            paid: true,
        })
        .await?;
    let query = GetEntitlement { lookup: handlers };
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
        "org-1".to_owned(),
    )?;
    let result = query
        .handle(request)
        .await
        .map_err(|error| error.message().to_owned())?;
    println!("{result:?}");
    Ok(())
}
