#![allow(clippy::panic_in_result_fn)]

#[path = "../examples/read_model/application.rs"]
mod application;

use std::{num::NonZeroU32, sync::Arc};

use application::{
    BillingChanged, DemoChanged, Entitlement, EntitlementLookup, Entitlements, ExampleResult,
    GetEntitlement, MemberJoined, OrganizationEvent, rebuild,
};
use rostfrei::{
    ApplicationName, DomainEventHandlerErrorKind, JsonReadModelCodec, QueryHandler,
    QueryHandlerRequest, ReadModelKey, ReadModelStore,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, StreamId, derive_commit_id, derive_event_id,
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

fn envelope_context() -> ExampleResult<EnvelopeContext> {
    Ok(EnvelopeContext::new(
        MessageId::new("billing-7")?,
        SchemaVersion::new(1)?,
        CorrelationId::new("example")?,
        None,
    ))
}

fn billing_delivery(
    context: &rostfrei::BoundedContext,
) -> ExampleResult<MessageDelivery<IntegrationEventAddress>> {
    let envelope = IntegrationEventEnvelope::new(
        envelope_context()?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: 7,
            paid: true,
        },
    )?;
    Ok(MessageDelivery::new(
        context.integration_event_address("billing-changed")?,
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
    let client = async_nats::connect(rostfrei_testing::integration::nats_url()?).await?;
    let jetstream = async_nats::jetstream::new(client);
    let context = ApplicationName::new(format!("read-model-projection-{}", std::process::id()))?
        .bounded_context("access")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?;
    provision_event_store(&jetstream, &events).await?;
    let history = NatsEventStore::connect(jetstream.clone(), events.clone()).await?;
    let config = NatsReadModelConfig::new(&context, "entitlements")?;
    provision_read_model(&jetstream, &config).await?;
    let store = Arc::new(
        NatsReadModelStore::connect(
            jetstream.clone(),
            config.clone(),
            JsonReadModelCodec::<Entitlement>::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let handlers = Arc::new(Entitlements::new(store.clone())?);
    let dispatcher = handlers.dispatcher()?;
    let stream = StreamId::new(
        AggregateType::new("organization")?,
        AggregateId::new("org-1")?,
    );
    let source_events = [
        OrganizationEvent::MemberJoined(MemberJoined),
        OrganizationEvent::DemoChanged(DemoChanged { active: true }),
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
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await?
        .into_events();
    let second = recorded.get(1).ok_or("missing event")?;
    assert_eq!(
        dispatcher.dispatch(second).await.unwrap_err().kind(),
        DomainEventHandlerErrorKind::OperatorBlocking
    );
    let key = ReadModelKey::new("org-1")?;
    assert!(store.read(&key).await?.is_none()); // A gap never advances the checkpoint.

    let (domain, integration) = tokio::join!(
        rebuild(&history, &stream, &dispatcher),
        MessageHandler::handle(handlers.as_ref(), billing_delivery(&context)?)
    );
    domain?;
    assert_eq!(integration, DeliveryDisposition::Acknowledge);
    let expected = Entitlement {
        members: 1,
        demo: true,
        paid: true,
        organization_version: 2,
        billing_version: 7,
    };
    assert_eq!(handlers.lookup("org-1").await?, Some(expected.clone()));
    let before_duplicates = store.read(&key).await?.ok_or("missing snapshot")?.revision;
    rebuild(&history, &stream, &dispatcher).await?;
    assert_eq!(
        MessageHandler::handle(handlers.as_ref(), billing_delivery(&context)?).await,
        DeliveryDisposition::Acknowledge
    );
    handlers
        .billing_changed(&BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: 6,
            paid: false,
        })
        .await?;
    assert_eq!(
        store.read(&key).await?.ok_or("missing snapshot")?.revision,
        before_duplicates
    );

    let query = GetEntitlement { lookup: handlers };
    let request = QueryHandlerRequest::new(
        envelope_context()?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        CallerMetadata::default(),
        None,
        "org-1".to_owned(),
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
    let next = NatsReadModelConfig::new(&context, "entitlements-rebuild")?;
    provision_read_model(&jetstream, &next).await?;
    let next_store = Arc::new(
        NatsReadModelStore::connect(
            jetstream.clone(),
            next.clone(),
            JsonReadModelCodec::<Entitlement>::new(NonZeroU32::MIN),
        )
        .await?,
    );
    let rebuilt = Arc::new(Entitlements::new(next_store)?);
    rebuild(&history, &stream, &rebuilt.dispatcher()?).await?;
    assert!(
        !rebuilt
            .lookup("org-1")
            .await?
            .ok_or("missing rebuilt snapshot")?
            .paid
    );
    // Billing is re-fetched from its authoritative owner; domain history alone
    // cannot recreate external-source facts.
    rebuilt
        .billing_changed(&BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: 7,
            paid: true,
        })
        .await?;
    assert_eq!(rebuilt.lookup("org-1").await?, Some(expected));
    for stream in [
        config.stream_name(),
        next.stream_name(),
        events.stream_name().to_owned(),
    ] {
        jetstream.delete_stream(stream).await?;
    }
    Ok(())
}
