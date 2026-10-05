//! `python3 scripts/test_nats.py -- cargo run --locked -p rostfrei-nats --example read_model`

mod application;
mod source;

use std::time::Duration;

use application::{GetAccess, OrganizationAccess};
use rostfrei::{ApplicationName, QueryHandler, QueryHandlerRequest, ReadModelKey, ReadModels};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, StreamId, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, EnvelopeContext, IntegrationEventEnvelope, MessageId,
    MessageTimestamp, OutboundMessage, SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig,
    NatsReadModelBackend, NatsReadModelConfig, NatsReadModelConsumerOptions, NatsReadModelWorker,
    connect, provision_application_messaging, provision_event_store, provision_read_model,
    provision_read_model_consumers,
};
use source::{BillingChanged, DemoChanged, MemberJoined, OrganizationEvent, OrganizationRenamed};

rostfrei::install_macro_support!();
type ExampleResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn envelope(id: &str) -> ExampleResult<EnvelopeContext> {
    Ok(EnvelopeContext::new(
        MessageId::new(id)?,
        SchemaVersion::new(1)?,
        CorrelationId::new("example")?,
        None,
    ))
}

#[tokio::main]
async fn main() -> ExampleResult {
    let connection = connect(&NatsConnectionConfig::new(
        "read-model-example",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new("read-model-runtime-example")?;
    let context = application.bounded_context("access")?;
    let billing = application.bounded_context("billing")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?;
    let messaging =
        ApplicationMessagingConfig::new(&application)?.with_max_bytes(8 * 1024 * 1024)?;
    let policy = NatsReadModelConfig::for_model::<OrganizationAccess>(&context)?;
    provision_event_store(connection.jetstream(), &events).await?;
    provision_application_messaging(connection.jetstream(), &messaging).await?;
    provision_read_model(connection.jetstream(), &policy).await?;

    let read_models = ReadModels::new(
        context,
        NatsReadModelBackend::new(connection.jetstream().clone()),
    );
    let model = application::register(&read_models, billing.clone()).await?;
    let options = NatsReadModelConsumerOptions::default();
    provision_read_model_consumers(
        connection.jetstream(),
        &model,
        &events,
        messaging.topology(),
        &options,
    )
    .await?;
    let worker = NatsReadModelWorker::connect(
        connection.jetstream().clone(),
        &model,
        &events,
        messaging.topology(),
        &options,
    )
    .await?;

    let history = NatsEventStore::connect(connection.jetstream().clone(), events).await?;
    seed_organization(&history).await?;
    let fact = IntegrationEventEnvelope::new(
        envelope("billing-7")?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: 7,
            paid: true,
        },
    )?;
    connection
        .publisher(messaging.topology().clone())
        .publish_integration_event_with_ack(
            OutboundMessage::json(
                billing.integration_event_address("billing-changed")?,
                fact.message_id().clone(),
                &fact,
            )?,
            Duration::from_secs(5),
        )
        .await?;

    let (_shutdown, receiver) = tokio::sync::watch::channel(false);
    tokio::select! {
        result = worker.run_until_shutdown(receiver) => {
            result?;
            return Err("read-model worker stopped unexpectedly".into());
        }
        result = tokio::time::timeout(Duration::from_secs(10), async {
            let key = ReadModelKey::new("org-1")?;
            loop {
                if model.reader().read(&key).await?.is_some_and(|view| view.members == 1 && view.demo && view.paid) { return ExampleResult::Ok(()); }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }) => { result??; }
    }
    let query = GetAccess {
        reader: model.reader(),
    };
    let request = QueryHandlerRequest::new(
        envelope("query-1")?,
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

async fn seed_organization(history: &NatsEventStore) -> ExampleResult {
    let stream = StreamId::new(
        AggregateType::new("organization")?,
        AggregateId::new("org-1")?,
    );
    let changes = [
        OrganizationEvent::MemberJoined(MemberJoined {
            organization_id: "org-1".to_owned(),
        }),
        // Intentionally unhandled: the runtime fills this source-version gap.
        OrganizationEvent::OrganizationRenamed(OrganizationRenamed {
            name: "Example".to_owned(),
        }),
        OrganizationEvent::DemoChanged(DemoChanged {
            organization_id: "org-1".to_owned(),
            active: true,
        }),
    ];
    let operation = OperationId::new("seed-example")?;
    let commit = derive_commit_id(&stream, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest("example"),
        changes
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
    history
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await?;
    Ok(())
}
