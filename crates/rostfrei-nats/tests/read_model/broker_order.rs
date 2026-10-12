use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use async_nats::jetstream::consumer::{self, PullConsumer};
use futures_util::TryStreamExt as _;
use rostfrei::{IntegrationEvent, IntegrationEventOrder, ReadModel, ReadModelRuntime, ReadModels};
use serde::{Deserialize, Serialize};

use super::{
    ApplicationMessagingConfig, BillingChanged as VersionedBilling, CallerMetadata,
    DeliveryDisposition, DeliveryInfo, Fixture, IntegrationEventEnvelope, MessageDelivery,
    MessageTimestamp, NatsPublisher, NatsReadModelConfig, NatsReadModelConsumerOptions,
    NatsReadModelWorker, OrganizationAccess, OutboundMessage, ReadModelKey, ReadModelStore,
    RebuiltAccess, TestResult, application, envelope,
    faults::{Backend, Faults},
    provision_application_messaging, provision_read_model, provision_read_model_consumers, source,
};

#[derive(Serialize, Deserialize)]
struct Revoked {
    organization_id: String,
}

impl IntegrationEvent for Revoked {
    const EVENT_NAME: &'static str = "billing-revoked";
    const SCHEMA_VERSION: u32 = 1;
    const BOUNDED_CONTEXT: Option<&'static str> = Some("billing");
}

#[derive(Serialize, Deserialize)]
struct LocalEvent {
    organization_id: String,
}

impl IntegrationEvent for LocalEvent {
    const EVENT_NAME: &'static str = "local-event";
    const SCHEMA_VERSION: u32 = 1;
}

async fn deliver_at<M: ReadModel, E: IntegrationEvent>(
    model: &ReadModelRuntime<M>,
    event: &E,
    id: &str,
    source_sequence: u64,
    consumer_sequence: u64,
) -> TestResult<DeliveryDisposition> {
    let binding = model
        .integration_bindings()
        .iter()
        .find(|binding| binding.is_event::<E>())
        .ok_or("missing binding")?;
    let event = IntegrationEventEnvelope::new(
        envelope(id)?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        event,
    )?;
    let delivery = MessageDelivery::new(
        binding.address().clone(),
        event.message_id().clone(),
        serde_json::to_vec(&event)?,
        CallerMetadata::default(),
        DeliveryInfo::new(1, 0, source_sequence, consumer_sequence)?,
    )?;
    Ok(binding.handler().handle(delivery).await)
}

#[tokio::test]
async fn simple_example_defaults_to_broker_sequence_and_declared_producer() -> TestResult {
    let fixture = Fixture::new().await?;
    let model =
        application::register(&ReadModels::new(fixture.context.clone(), fixture.backend())).await?;
    let binding = model.integration_bindings().first().ok_or("missing")?;
    assert!(binding.is_broker_ordered());
    assert_eq!(
        binding.address(),
        &fixture
            .billing
            .integration_event_address("billing-changed")?
    );
    let event = source::BillingChanged {
        organization_id: "org-1".to_owned(),
        paid: true,
    };
    assert_eq!(
        deliver_at(&model, &event, "first", 400, 1).await?,
        DeliveryDisposition::Acknowledge
    );
    let key = ReadModelKey::new("org-1")?;
    let revision = fixture
        .state()
        .await?
        .read(&key)
        .await?
        .ok_or("missing")?
        .revision;
    assert_eq!(
        deliver_at(&model, &event, "first", 400, 2).await?,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(
        fixture
            .state()
            .await?
            .read(&key)
            .await?
            .ok_or("missing")?
            .revision,
        revision
    );
    let later = source::BillingChanged {
        organization_id: "org-1".to_owned(),
        paid: false,
    };
    assert_eq!(
        deliver_at(&model, &later, "later", 401, 3).await?,
        DeliveryDisposition::Acknowledge
    );
    assert!(!model.reader().read(&key).await?.ok_or("missing")?.paid);
    fixture.cleanup().await
}

#[tokio::test]
async fn business_versions_override_arrival_order_when_explicitly_requested() -> TestResult {
    let fixture = Fixture::new().await?;
    let models = ReadModels::new(fixture.context.clone(), fixture.backend());
    let broker = models
        .register::<OrganizationAccess>()
        .on_integration_event::<VersionedBilling>(
            |event| event.organization_id.clone(),
            |view, event| view.paid = event.paid,
        )
        .build()
        .await?;
    let policy = NatsReadModelConfig::for_model::<RebuiltAccess>(&fixture.context)?;
    provision_read_model(&fixture.js, &policy).await?;
    let business = models
        .register::<RebuiltAccess>()
        .on_integration_event::<VersionedBilling>(
            |event| event.organization_id.clone(),
            |view, event| view.0.paid = event.paid,
        )
        .integration_source::<VersionedBilling>(
            fixture.billing.clone(),
            IntegrationEventOrder::<VersionedBilling>::latest("billing", |event| {
                event.source_version
            }),
        )
        .build()
        .await?;
    assert!(
        !business
            .integration_bindings()
            .first()
            .ok_or("missing")?
            .is_broker_ordered()
    );
    for (id, source_sequence, consumer_sequence, version, paid) in [
        ("new-fact", 10, 1, 8, false),
        ("late-old-fact", 11, 2, 7, true),
    ] {
        let event = VersionedBilling {
            organization_id: "org-1".to_owned(),
            source_version: version,
            paid,
        };
        assert_eq!(
            deliver_at(&broker, &event, id, source_sequence, consumer_sequence).await?,
            DeliveryDisposition::Acknowledge
        );
        assert_eq!(
            deliver_at(&business, &event, id, source_sequence, consumer_sequence).await?,
            DeliveryDisposition::Acknowledge
        );
    }
    let key = ReadModelKey::new("org-1")?;
    assert!(broker.reader().read(&key).await?.ok_or("missing")?.paid);
    assert!(!business.reader().read(&key).await?.ok_or("missing")?.0.paid);
    fixture.js.delete_stream(policy.stream_name()).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn undeclared_producers_default_locally_and_allow_a_context_only_override() -> TestResult {
    let fixture = Fixture::new().await?;
    let models = ReadModels::new(fixture.context.clone(), fixture.backend());
    let local = models
        .register::<OrganizationAccess>()
        .on_integration_event::<LocalEvent>(|event| event.organization_id.clone(), |_, _| {})
        .build()
        .await?;
    assert_eq!(
        local
            .integration_bindings()
            .first()
            .ok_or("missing")?
            .address(),
        &fixture.context.integration_event_address("local-event")?
    );
    let other = ReadModels::new(fixture.context.clone(), fixture.backend())
        .register::<OrganizationAccess>()
        .on_integration_event::<LocalEvent>(|event| event.organization_id.clone(), |_, _| {})
        .integration_context::<LocalEvent>(fixture.billing.clone())
        .build()
        .await?;
    assert_eq!(
        other
            .integration_bindings()
            .first()
            .ok_or("missing")?
            .address(),
        &fixture.billing.integration_event_address("local-event")?
    );
    let wrong = ReadModels::new(fixture.context.clone(), fixture.backend())
        .register::<OrganizationAccess>()
        .on_integration_event::<source::BillingChanged>(
            |event| event.organization_id.clone(),
            |_, _| {},
        )
        .integration_context::<source::BillingChanged>(fixture.context.clone())
        .build()
        .await;
    assert!(matches!(
        wrong,
        Err(rostfrei::ReadModelProcessingError::Configuration(_))
    ));
    fixture.cleanup().await
}

async fn publish<E: IntegrationEvent>(
    fixture: &Fixture,
    messaging: &ApplicationMessagingConfig,
    event: E,
    id: &str,
) -> TestResult<rostfrei_nats::NatsPublishAck> {
    let envelope = IntegrationEventEnvelope::new(
        envelope(id)?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        event,
    )?;
    Ok(
        NatsPublisher::new(fixture.js.clone(), messaging.topology().clone())
            .publish_integration_event_with_ack(
                OutboundMessage::json(
                    fixture.billing.integration_event_address(E::EVENT_NAME)?,
                    envelope.message_id().clone(),
                    &envelope,
                )?,
                Duration::from_secs(5),
            )
            .await?,
    )
}

async fn inspect_consumer(
    fixture: &Fixture,
    messaging: &ApplicationMessagingConfig,
) -> TestResult<PullConsumer> {
    let stream = fixture
        .js
        .get_stream(messaging.topology().integration_event_stream().as_str())
        .await?;
    let names: Vec<String> = stream.consumer_names().try_collect().await?;
    assert_eq!(names.len(), 1);
    stream.get_consumer(names.first().ok_or("missing")?).await
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn broker_worker_preserves_cross_type_order_through_retries_and_lost_acknowledgements()
-> TestResult {
    let fixture = Fixture::new().await?;
    let faults = Arc::new(Faults::default());
    faults.unavailable_once.store(true, Ordering::SeqCst);
    faults.ambiguous.store(true, Ordering::SeqCst);
    let model = ReadModels::new(
        fixture.context.clone(),
        Backend {
            inner: fixture.backend(),
            faults: faults.clone(),
        },
    )
    .register::<OrganizationAccess>()
    .on_integration_event::<source::BillingChanged>(
        |event| event.organization_id.clone(),
        |view, event| {
            view.members = view.members.saturating_add(1);
            view.paid = event.paid;
        },
    )
    .on_integration_event::<Revoked>(
        |event| event.organization_id.clone(),
        |view, _| {
            view.members = view.members.saturating_add(1);
            view.paid = false;
        },
    )
    .build()
    .await?;
    let messaging = ApplicationMessagingConfig::new(fixture.context.application())?
        .with_max_bytes(4 * 1024 * 1024)?;
    provision_application_messaging(&fixture.js, &messaging).await?;
    let options = NatsReadModelConsumerOptions::default();
    provision_read_model_consumers(
        &fixture.js,
        &model,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    let inspector = inspect_consumer(&fixture, &messaging).await?;
    assert_eq!(inspector.cached_info().config.filter_subjects.len(), 2);
    assert_eq!(inspector.cached_info().config.max_ack_pending, 1);
    let worker = NatsReadModelWorker::connect(
        fixture.js.clone(),
        &model,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    let first = publish(
        &fixture,
        &messaging,
        source::BillingChanged {
            organization_id: "org-1".to_owned(),
            paid: true,
        },
        "grant",
    )
    .await?;
    let second = publish(
        &fixture,
        &messaging,
        Revoked {
            organization_id: "org-1".to_owned(),
        },
        "revoke",
    )
    .await?;
    assert!(second.sequence() > first.sequence());
    let (_stop, shutdown) = tokio::sync::watch::channel(false);
    let result = tokio::select! {
        result = worker.run_until_shutdown(shutdown) => { result?; Err("worker ended".into()) }
        result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let info = inspector.get_info().await?;
                if info.ack_floor.stream_sequence >= second.sequence() && info.num_ack_pending == 0 { return TestResult::Ok(()); }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }) => result?,
    };
    result?;
    let key = ReadModelKey::new("org-1")?;
    let view = model.reader().read(&key).await?.ok_or("missing")?;
    assert_eq!(view.members, 2);
    assert!(!view.paid);
    assert_eq!(faults.committed.load(Ordering::SeqCst), 2);
    assert!(inspector.get_info().await?.delivered.consumer_sequence >= 4);

    let info = inspector.get_info().await?;
    let actual = info.config;
    fixture
        .js
        .create_consumer_on_stream(
            consumer::pull::Config {
                durable_name: actual.durable_name,
                name: actual.name,
                description: actual.description,
                deliver_policy: actual.deliver_policy,
                ack_policy: actual.ack_policy,
                ack_wait: actual.ack_wait,
                max_deliver: -1,
                filter_subjects: actual.filter_subjects,
                max_ack_pending: 2,
                max_batch: 1,
                ..Default::default()
            },
            messaging.topology().integration_event_stream().as_str(),
        )
        .await?;
    assert!(
        matches!(NatsReadModelWorker::connect(fixture.js.clone(), &model, &fixture.events, messaging.topology(), &options).await, Err(rostfrei_nats::ReadModelWorkerError::Integration(error)) if error.kind() == rostfrei_messaging_core::ConsumeErrorKind::InvalidConfiguration)
    );
    for stream in messaging.streams() {
        fixture.js.delete_stream(stream.name().as_str()).await?;
    }
    fixture.cleanup().await
}
