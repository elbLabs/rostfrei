#![allow(clippy::panic_in_result_fn)]

#[path = "../examples/read_model/application.rs"]
mod application;
#[path = "read_model/runtime_faults.rs"]
mod faults;
#[path = "../examples/read_model/source.rs"]
mod source;

use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use application::{GetAccess, OrganizationAccess};
use async_nats::jetstream;
use rostfrei::{
    ApplicationName, BoundedContext, IntegrationEventOrder, JsonReadModelCodec, QueryHandler,
    QueryHandlerRequest, ReadModelKey, ReadModelProcessingError, ReadModelRuntime, ReadModelState,
    ReadModelStore, ReadModels, TrafficScope,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, Event, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, DeliveryDisposition, DeliveryInfo, EnvelopeContext,
    IntegrationEventEnvelope, MessageDelivery, MessageId, MessageTimestamp, OutboundMessage,
    SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsEventStore, NatsEventStoreConfig, NatsPublisher,
    NatsReadModelBackend, NatsReadModelConfig, NatsReadModelConsumerOptions, NatsReadModelStore,
    NatsReadModelWorker, provision_application_messaging, provision_event_store,
    provision_read_model, provision_read_model_consumers,
};
use source::{
    BillingChanged, DemoChanged, MemberJoined, Organization, OrganizationEvent, OrganizationRenamed,
};

rostfrei::install_macro_support!();
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Default, serde::Serialize, serde::Deserialize, rostfrei::ReadModel)]
#[read_model(id = "organization-count", version = 1)]
struct OrganizationCount {
    members: u64,
}

#[derive(Default, serde::Serialize, serde::Deserialize, rostfrei::ReadModel)]
#[read_model(id = "organization-access-rebuilt", version = 1)]
struct RebuiltAccess(OrganizationAccess);

struct SecondaryOrganization;
impl rostfrei_core::Aggregate for SecondaryOrganization {
    type State = ();
    type Event = OrganizationEvent;
    const BOUNDED_CONTEXT: &'static str = "access";
    const AGGREGATE_TYPE: &'static str = "secondary-organization";
    fn initial(_: &StreamId) {}
    fn apply(_state: &mut (), _event: &OrganizationEvent) {}
}

struct Fixture {
    js: jetstream::Context,
    context: BoundedContext,
    billing: BoundedContext,
    events: NatsEventStoreConfig,
    model: NatsReadModelConfig,
    history: NatsEventStore,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let js =
            jetstream::new(async_nats::connect(rostfrei_testing::integration::nats_url()?).await?);
        let app = ApplicationName::new(format!(
            "read-runtime-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))?;
        let context = app.bounded_context("access")?;
        let billing = app.bounded_context("billing")?;
        let events = NatsEventStoreConfig::for_bounded_context(&context)?
            .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        provision_event_store(&js, &events).await?;
        let history = NatsEventStore::connect(js.clone(), events.clone()).await?;
        let model = NatsReadModelConfig::for_model::<OrganizationAccess>(&context)?;
        provision_read_model(&js, &model).await?;
        Ok(Self {
            js,
            context,
            billing,
            events,
            model,
            history,
        })
    }

    fn backend(&self) -> NatsReadModelBackend {
        NatsReadModelBackend::new(self.js.clone()).with_event_store_config(self.events.clone())
    }

    async fn runtime(&self) -> TestResult<ReadModelRuntime<OrganizationAccess>> {
        Ok(application::register(
            &ReadModels::new(self.context.clone(), self.backend()),
            self.billing.clone(),
        )
        .await?)
    }

    async fn state(
        &self,
    ) -> TestResult<
        NatsReadModelStore<
            ReadModelState<OrganizationAccess>,
            JsonReadModelCodec<ReadModelState<OrganizationAccess>>,
        >,
    > {
        Ok(NatsReadModelStore::connect(
            self.js.clone(),
            self.model.clone(),
            JsonReadModelCodec::new(NonZeroU32::MIN),
        )
        .await?)
    }

    async fn seed(&self, changes: Vec<OrganizationEvent>) -> TestResult<Vec<RecordedEvent>> {
        self.seed_stream("organization", "org-1", changes).await
    }

    async fn seed_stream(
        &self,
        aggregate: &str,
        id: &str,
        changes: Vec<OrganizationEvent>,
    ) -> TestResult<Vec<RecordedEvent>> {
        let stream = StreamId::new(AggregateType::new(aggregate)?, AggregateId::new(id)?);
        let prior = self.history.load(&stream).await?;
        let operation = OperationId::new(format!("seed-{aggregate}-{id}-{}", prior.len()))?;
        let commit = derive_commit_id(&stream, &operation);
        let expected = if prior.is_empty() {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(u64::try_from(prior.len())?))
        };
        let batch = EventBatch::new(
            commit.clone(),
            operation,
            ContentFingerprint::digest("runtime-events"),
            changes
                .iter()
                .enumerate()
                .map(|(ordinal, event)| {
                    Ok(NewEvent::new(
                        derive_event_id(&commit, u32::try_from(ordinal)?),
                        event.event_type(),
                        1,
                        event.encode_json()?,
                    )?)
                })
                .collect::<TestResult<Vec<_>>>()?,
        )?;
        Ok(self
            .history
            .append(&stream, expected, batch)
            .await?
            .into_events())
    }

    async fn cleanup(&self) -> TestResult {
        self.js.delete_stream(self.model.stream_name()).await?;
        self.js.delete_stream(self.events.stream_name()).await?;
        Ok(())
    }
}

fn envelope(id: &str) -> TestResult<EnvelopeContext> {
    Ok(EnvelopeContext::new(
        MessageId::new(id)?,
        SchemaVersion::new(1)?,
        CorrelationId::new("runtime")?,
        None,
    ))
}

fn fact(
    id: &str,
    version: u64,
    paid: bool,
) -> TestResult<IntegrationEventEnvelope<BillingChanged>> {
    Ok(IntegrationEventEnvelope::new(
        envelope(id)?,
        MessageTimestamp::from_unix_milliseconds(1)?,
        BillingChanged {
            organization_id: "org-1".to_owned(),
            source_version: version,
            paid,
        },
    )?)
}

async fn deliver<M: rostfrei::ReadModel>(
    model: &ReadModelRuntime<M>,
    id: &str,
    version: u64,
    paid: bool,
) -> TestResult<DeliveryDisposition> {
    let binding = model
        .integration_bindings()
        .iter()
        .find(|binding| binding.is_event::<BillingChanged>())
        .ok_or("missing binding")?;
    let event = fact(id, version, paid)?;
    let delivery = MessageDelivery::new(
        binding.address().clone(),
        event.message_id().clone(),
        serde_json::to_vec(&event)?,
        CallerMetadata::default(),
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?;
    Ok(binding.handler().handle(delivery).await)
}

fn joined(id: &str) -> OrganizationEvent {
    OrganizationEvent::MemberJoined(MemberJoined {
        organization_id: id.to_owned(),
    })
}
fn demo(id: &str) -> OrganizationEvent {
    OrganizationEvent::DemoChanged(DemoChanged {
        organization_id: id.to_owned(),
        active: true,
    })
}

#[tokio::test]
async fn concurrent_handlers_catch_up_filtered_history_and_share_one_checkpointed_value()
-> TestResult {
    let fixture = Fixture::new().await?;
    let model = fixture.runtime().await?;
    let records = fixture
        .seed(vec![
            joined("org-1"),
            OrganizationEvent::OrganizationRenamed(OrganizationRenamed {
                name: "ignored".to_owned(),
            }),
            demo("org-1"),
        ])
        .await?;
    let first = records.first().ok_or("missing first")?;
    let last = records.last().ok_or("missing last")?;
    let (late, early, integration) = tokio::join!(
        model.dispatch_domain(last),
        model.dispatch_domain(first),
        deliver(&model, "billing-7", 7, true)
    );
    late?;
    early?;
    assert_eq!(integration?, DeliveryDisposition::Acknowledge);
    let key = ReadModelKey::new("org-1")?;
    let expected = OrganizationAccess {
        members: 1,
        demo: true,
        paid: true,
    };
    assert_eq!(model.reader().read(&key).await?, Some(expected));
    let revision = fixture
        .state()
        .await?
        .read(&key)
        .await?
        .ok_or("missing")?
        .revision;
    for event in &records {
        model.dispatch_domain(event).await?;
    }
    assert_eq!(
        deliver(&model, "billing-7-again", 7, false).await?,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(
        deliver(&model, "billing-6", 6, false).await?,
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

    // Fresh runtime/connection retains checkpoints and the reader only returns business fields.
    let model = fixture.runtime().await?;
    model.dispatch_domain(first).await?;
    let query = GetAccess {
        reader: model.reader(),
    };
    let result = query
        .handle(QueryHandlerRequest::new(
            envelope("query")?,
            MessageTimestamp::from_unix_milliseconds(1)?,
            CallerMetadata::default(),
            None,
            "org-1".to_owned(),
        )?)
        .await
        .map_err(|error| error.message().to_owned())?;
    assert_eq!(
        serde_json::to_value(result)?,
        serde_json::json!({"members":1,"demo":true,"paid":true})
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn catch_up_routes_only_events_for_the_selected_key() -> TestResult {
    let fixture = Fixture::new().await?;
    let model = fixture.runtime().await?;
    let records = fixture
        .seed(vec![joined("org-1"), joined("org-2"), demo("org-1")])
        .await?;
    model
        .dispatch_domain(records.last().ok_or("missing last")?)
        .await?;
    assert_eq!(
        model
            .reader()
            .read(&ReadModelKey::new("org-1")?)
            .await?
            .ok_or("missing")?
            .members,
        1
    );
    assert!(
        model
            .reader()
            .read(&ReadModelKey::new("org-2")?)
            .await?
            .is_none()
    );
    model
        .dispatch_domain(records.get(1).ok_or("missing second")?)
        .await?;
    assert_eq!(
        model
            .reader()
            .read(&ReadModelKey::new("org-2")?)
            .await?
            .ok_or("missing")?
            .members,
        1
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn checkpoints_are_independent_for_aggregate_types_and_identities() -> TestResult {
    let fixture = Fixture::new().await?;
    let model = ReadModels::new(fixture.context.clone(), fixture.backend())
        .register::<OrganizationAccess>()
        .from_aggregate::<Organization>()
        .on_domain_event::<MemberJoined>(
            |event| event.organization_id.clone(),
            |view, _| view.members = view.members.saturating_add(1),
        )
        .from_aggregate::<SecondaryOrganization>()
        .on_domain_event::<MemberJoined>(
            |event| event.organization_id.clone(),
            |view, _| view.members = view.members.saturating_add(10),
        )
        .build()
        .await?;
    for (aggregate, id) in [
        ("organization", "org-1"),
        ("secondary-organization", "org-1"),
        ("organization", "org-2"),
    ] {
        let records = fixture
            .seed_stream(aggregate, id, vec![joined("org-1")])
            .await?;
        let first = records.first().ok_or("missing")?;
        model.dispatch_domain(first).await?;
        model.dispatch_domain(first).await?;
    }
    assert_eq!(
        model
            .reader()
            .read(&ReadModelKey::new("org-1")?)
            .await?
            .ok_or("missing")?
            .members,
        12
    );
    fixture.cleanup().await
}

#[tokio::test]
async fn runtime_rejects_legacy_or_incompatible_checkpoint_envelopes() -> TestResult {
    let fixture = Fixture::new().await?;
    let model = fixture.runtime().await?;
    let raw = fixture
        .js
        .get_key_value(fixture.model.bucket_name())
        .await?;
    let key = ReadModelKey::new("org-1")?;
    let view = serde_json::json!({"members":0,"demo":false,"paid":false});
    for value in [
        serde_json::json!({"schema_version":1,"value":view}),
        serde_json::json!({"schema_version":1,"value":{"format_version":2,"value":view,"positions":{}}}),
        serde_json::json!({"schema_version":1,"value":{"format_version":1,"value":view,"positions":{"bad":0}}}),
    ] {
        let bytes = serde_json::to_vec(&value)?;
        let revision = raw.put(key.as_str(), bytes.into()).await?;
        assert_eq!(
            model.reader().read(&key).await.unwrap_err().kind(),
            rostfrei::ReadModelErrorKind::InvalidData
        );
        assert!(matches!(
            deliver(&model, "invalid-state", 7, true).await?,
            DeliveryDisposition::Quarantine(_)
        ));
        assert_eq!(
            raw.entry(key.as_str()).await?.ok_or("missing")?.revision,
            revision
        );
    }
    fixture.cleanup().await
}

#[tokio::test]
async fn unavailable_gap_history_does_not_persist_partial_state() -> TestResult {
    let fixture = Fixture::new().await?;
    let model = fixture.runtime().await?;
    let records = fixture.seed(vec![joined("org-1"), demo("org-1")]).await?;
    fixture
        .js
        .delete_stream(fixture.events.stream_name())
        .await?;
    let error = model
        .dispatch_domain(records.last().ok_or("missing")?)
        .await
        .unwrap_err();
    assert_eq!(
        error.kind(),
        rostfrei::DomainEventHandlerErrorKind::Retryable
    );
    assert!(
        model
            .reader()
            .read(&ReadModelKey::new("org-1")?)
            .await?
            .is_none()
    );
    provision_event_store(&fixture.js, &fixture.events).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn rebuild_uses_an_independent_model_generation_and_external_source_refresh() -> TestResult {
    let fixture = Fixture::new().await?;
    let serving = fixture.runtime().await?;
    let records = fixture.seed(vec![joined("org-1"), demo("org-1")]).await?;
    serving
        .dispatch_domain(records.last().ok_or("missing")?)
        .await?;
    assert_eq!(
        deliver(&serving, "billing", 7, true).await?,
        DeliveryDisposition::Acknowledge
    );
    let key = ReadModelKey::new("org-1")?;
    let serving_revision = fixture
        .state()
        .await?
        .read(&key)
        .await?
        .ok_or("missing")?
        .revision;

    let policy = NatsReadModelConfig::for_model::<RebuiltAccess>(&fixture.context)?;
    provision_read_model(&fixture.js, &policy).await?;
    let rebuilt = ReadModels::new(fixture.context.clone(), fixture.backend())
        .register::<RebuiltAccess>()
        .from_aggregate::<Organization>()
        .on_domain_event::<MemberJoined>(
            |event| event.organization_id.clone(),
            |view, _| view.0.members = view.0.members.saturating_add(1),
        )
        .on_domain_event::<DemoChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.0.demo = event.active,
        )
        .on_integration_event::<BillingChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.0.paid = event.paid,
        )
        .integration_source::<BillingChanged>(
            fixture.billing.clone(),
            IntegrationEventOrder::<BillingChanged>::latest("billing-account", |event| {
                event.source_version
            }),
        )
        .build()
        .await?;
    let history = fixture
        .history
        .load(records.first().ok_or("missing")?.stream_id())
        .await?;
    for event in &history {
        rebuilt.dispatch_domain(event).await?;
    }
    assert!(!rebuilt.reader().read(&key).await?.ok_or("missing")?.0.paid);
    assert_eq!(
        deliver(&rebuilt, "refresh", 7, true).await?,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(
        rebuilt.reader().read(&key).await?.ok_or("missing")?.0,
        serving.reader().read(&key).await?.ok_or("missing")?
    );
    assert_eq!(
        fixture
            .state()
            .await?
            .read(&key)
            .await?
            .ok_or("missing")?
            .revision,
        serving_revision
    );
    fixture.js.delete_stream(policy.stream_name()).await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn integration_ordering_is_explicit_and_consecutive_gaps_do_not_persist() -> TestResult {
    let fixture = Fixture::new().await?;
    let models = ReadModels::new(fixture.context.clone(), fixture.backend());
    let missing = models
        .register::<OrganizationAccess>()
        .on_integration_event::<BillingChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.paid = event.paid,
        )
        .build()
        .await;
    assert!(matches!(
        missing,
        Err(ReadModelProcessingError::Configuration(_))
    ));
    let model = models
        .register::<OrganizationAccess>()
        .on_integration_event::<BillingChanged>(
            |event| event.organization_id.clone(),
            |view, event| view.paid = event.paid,
        )
        .integration_source::<BillingChanged>(
            fixture.billing.clone(),
            IntegrationEventOrder::<BillingChanged>::consecutive("billing", |event| {
                event.source_version
            }),
        )
        .build()
        .await?;
    assert!(matches!(
        deliver(&model, "gap", 2, true).await?,
        DeliveryDisposition::Quarantine(_)
    ));
    let key = ReadModelKey::new("org-1")?;
    assert!(model.reader().read(&key).await?.is_none());
    assert_eq!(
        deliver(&model, "one", 1, true).await?,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(
        deliver(&model, "two", 2, false).await?,
        DeliveryDisposition::Acknowledge
    );
    assert!(!model.reader().read(&key).await?.ok_or("missing")?.paid);
    assert!(matches!(
        deliver(&model, "zero", 0, true).await?,
        DeliveryDisposition::Quarantine(_)
    ));
    fixture.cleanup().await
}

#[tokio::test]
async fn registration_rejects_duplicates_wrong_contexts_and_foreign_traffic() -> TestResult {
    let fixture = Fixture::new().await?;
    let models = ReadModels::new(fixture.context.clone(), fixture.backend());
    let duplicate = models
        .register::<OrganizationAccess>()
        .from_aggregate::<Organization>()
        .on_domain_event::<MemberJoined>(|event| event.organization_id.clone(), |_, _| {})
        .on_domain_event::<MemberJoined>(|event| event.organization_id.clone(), |_, _| {})
        .build()
        .await;
    assert!(matches!(
        duplicate,
        Err(ReadModelProcessingError::Configuration(_))
    ));
    let foreign = fixture
        .context
        .application()
        .bounded_context_in_scope(TrafficScope::Test, "billing")?;
    let result = models
        .register::<OrganizationAccess>()
        .on_integration_event::<BillingChanged>(|event| event.organization_id.clone(), |_, _| {})
        .integration_source::<BillingChanged>(
            foreign,
            IntegrationEventOrder::<BillingChanged>::latest("billing", |event| {
                event.source_version
            }),
        )
        .build()
        .await;
    assert!(matches!(
        result,
        Err(ReadModelProcessingError::Configuration(_))
    ));
    let wrong = ReadModels::new(fixture.billing.clone(), fixture.backend())
        .register::<OrganizationAccess>()
        .from_aggregate::<Organization>()
        .on_domain_event::<MemberJoined>(|event| event.organization_id.clone(), |_, _| {})
        .build()
        .await;
    assert!(matches!(
        wrong,
        Err(ReadModelProcessingError::Configuration(_))
    ));
    let _registered = application::register(&models, fixture.billing.clone()).await?;
    let duplicate_model = application::register(&models, fixture.billing.clone()).await;
    assert!(matches!(
        duplicate_model,
        Err(ReadModelProcessingError::Configuration(_))
    ));
    fixture.cleanup().await
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn worker_runs_both_event_families_and_materializes_before_ack() -> TestResult {
    let fixture = Fixture::new().await?;
    let messaging = ApplicationMessagingConfig::new(fixture.context.application())?
        .with_max_bytes(4 * 1024 * 1024)?;
    provision_application_messaging(&fixture.js, &messaging).await?;
    let model = fixture.runtime().await?;
    let options = NatsReadModelConsumerOptions::default();
    let count_policy = NatsReadModelConfig::for_model::<OrganizationCount>(&fixture.context)?;
    provision_read_model(&fixture.js, &count_policy).await?;
    let counts = ReadModels::new(fixture.context.clone(), fixture.backend())
        .register::<OrganizationCount>()
        .from_aggregate::<Organization>()
        .on_domain_event::<MemberJoined>(
            |event| {
                event
                    .recorded()
                    .stream_id()
                    .aggregate_id()
                    .as_str()
                    .to_owned()
            },
            |view, _| view.members = view.members.saturating_add(1),
        )
        .build()
        .await?;
    provision_read_model_consumers(
        &fixture.js,
        &counts,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    let count_worker = NatsReadModelWorker::connect(
        fixture.js.clone(),
        &counts,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    provision_read_model_consumers(
        &fixture.js,
        &model,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    let worker = NatsReadModelWorker::connect(
        fixture.js.clone(),
        &model,
        &fixture.events,
        messaging.topology(),
        &options,
    )
    .await?;
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let count_worker = tokio::spawn(count_worker.run_until_shutdown(receiver.clone()));
    let worker = tokio::spawn(worker.run_until_shutdown(receiver));
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        fixture.seed(vec![joined("org-1"), demo("org-1")]).await?;
        let fact = fact("worker-billing", 7, true)?;
        NatsPublisher::new(fixture.js.clone(), messaging.topology().clone())
            .publish_integration_event_with_ack(
                OutboundMessage::json(
                    fixture
                        .billing
                        .integration_event_address("billing-changed")?,
                    fact.message_id().clone(),
                    &fact,
                )?,
                Duration::from_secs(5),
            )
            .await?;
        loop {
            if model
                .reader()
                .read(&ReadModelKey::new("org-1")?)
                .await?
                .is_some_and(|view| view.members == 1 && view.demo && view.paid)
                && counts
                    .reader()
                    .read(&ReadModelKey::new("org-1")?)
                    .await?
                    .is_some_and(|view| view.members == 1)
            {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    shutdown.send(true)?;
    worker.await??;
    count_worker.await??;
    result??;
    for stream in messaging.streams() {
        fixture.js.delete_stream(stream.name().as_str()).await?;
    }
    fixture.js.delete_stream(count_policy.stream_name()).await?;
    fixture.cleanup().await
}
