#![allow(clippy::panic_in_result_fn)]

mod native_connection_support;

use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream::{self, stream::RetentionPolicy};
use futures_util::TryStreamExt as _;
use rostfrei::{
    ApplicationName, BoundedContext, CorrelationChain, CorrelationIndexFreshness,
    CorrelationIndexReader, CorrelationRef, CorrelationSourceKind, CorrelationSourceState,
    MAX_CORRELATION_REFS, ReadModelError, ReadModelErrorKind, TrafficScope,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{
    CausationId, CorrelationId, EnvelopeContext, IntegrationEventEnvelope, MessageId,
    MessageTimestamp, OutboundMessage, SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsCorrelationIndexProjector, NatsCorrelationIndexReader,
    NatsEventStore, NatsEventStoreConfig, NatsPublisher, NatsReadModelConfig,
    provision_application_messaging, provision_event_store, provision_read_model,
};
use tokio::{sync::watch, task::JoinHandle};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const WAIT: Duration = Duration::from_secs(20);

struct Context {
    scope: BoundedContext,
    events: NatsEventStoreConfig,
    store: NatsEventStore,
}

struct Fixture {
    js: jetstream::Context,
    messaging: ApplicationMessagingConfig,
    index: NatsReadModelConfig,
    ordering: Context,
    billing: Context,
}

struct Running {
    shutdown: watch::Sender<bool>,
    task: JoinHandle<Result<(), ReadModelError>>,
}

impl Running {
    async fn stop(self) -> TestResult {
        self.shutdown.send(true)?;
        self.task.await??;
        Ok(())
    }
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        Self::with_index_limits(16 * 1024 * 1024, 256 * 1024).await
    }

    async fn with_index_limits(max_bytes: i64, max_value_bytes: i32) -> TestResult<Self> {
        let js = jetstream::new(
            native_connection_support::client(rostfrei_testing::integration::nats_url()?).await?,
        );
        let application = ApplicationName::new(format!(
            "correlation-index-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))?;
        let messaging =
            ApplicationMessagingConfig::new(&application)?.with_max_bytes(16 * 1024 * 1024)?;
        provision_application_messaging(&js, &messaging).await?;
        let index = NatsReadModelConfig::correlation_index(&application, TrafficScope::Normal)?
            .with_storage_limits(max_bytes, max_value_bytes)?;
        provision_read_model(&js, &index).await?;
        Ok(Self {
            ordering: Context::new(&js, application.bounded_context("ordering")?).await?,
            billing: Context::new(&js, application.bounded_context("billing")?).await?,
            js,
            messaging,
            index,
        })
    }

    async fn start(&self, context: &Context) -> TestResult<Running> {
        let projector = NatsCorrelationIndexProjector::connect(
            self.js.clone(),
            &self.index,
            context.events.clone(),
            self.messaging.topology(),
        )
        .await?;
        Ok(Running::spawn(projector))
    }

    fn integration_stream(&self) -> String {
        self.messaging
            .topology()
            .integration_event_stream()
            .as_str()
            .to_owned()
    }

    /// Recreates a stream with a changed policy; NATS rejects most in-place edits.
    async fn recreate_stream(
        &self,
        name: &str,
        change: impl FnOnce(&mut jetstream::stream::Config),
    ) -> TestResult {
        let mut config = self.js.get_stream(name).await?.cached_info().config.clone();
        self.js.delete_stream(name).await?;
        change(&mut config);
        self.js.create_stream(config).await?;
        Ok(())
    }

    async fn projector_consumers(&self, stream: &str) -> TestResult<Vec<String>> {
        let names: Vec<String> = self
            .js
            .get_stream(stream)
            .await?
            .consumer_names()
            .try_collect()
            .await?;
        Ok(names
            .into_iter()
            .filter(|name| name.starts_with("correlation-index--"))
            .collect())
    }
}

impl Running {
    fn spawn(projector: NatsCorrelationIndexProjector) -> Self {
        let (shutdown, receiver) = watch::channel(false);
        Self {
            shutdown,
            task: tokio::spawn(projector.run_until_shutdown(receiver)),
        }
    }
}

impl Fixture {
    async fn reader(&self) -> TestResult<NatsCorrelationIndexReader> {
        Ok(NatsCorrelationIndexReader::connect(self.js.clone(), &self.index).await?)
    }

    async fn publish(
        &self,
        context: &Context,
        name: &str,
        correlation: &str,
        cause: &RecordedEvent,
    ) -> TestResult<(MessageId, u64)> {
        let message_id = MessageId::new(format!("{name}-{correlation}"))?;
        let envelope = IntegrationEventEnvelope::new(
            EnvelopeContext::new(
                message_id.clone(),
                SchemaVersion::new(1)?,
                CorrelationId::new(correlation)?,
                Some(CausationId::new(cause.event_id().as_str())?),
            ),
            now()?,
            serde_json::json!({ "order_id": "O1" }),
        )?;
        let ack = NatsPublisher::new(self.js.clone(), self.messaging.topology().clone())
            .publish_integration_event_with_ack(
                OutboundMessage::json(
                    context.scope.integration_event_address(name)?,
                    message_id.clone(),
                    &envelope,
                )?,
                Duration::from_secs(5),
            )
            .await?;
        Ok((message_id, ack.sequence()))
    }

    async fn cleanup(&self) -> TestResult {
        for stream in self.messaging.streams() {
            self.js.delete_stream(stream.name().as_str()).await?;
        }
        for context in [&self.ordering, &self.billing] {
            self.js.delete_stream(context.events.stream_name()).await?;
        }
        self.js.delete_stream(self.index.stream_name()).await?;
        Ok(())
    }
}

impl Context {
    async fn new(js: &jetstream::Context, scope: BoundedContext) -> TestResult<Self> {
        let events = NatsEventStoreConfig::for_bounded_context(&scope)?
            .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        provision_event_store(js, &events).await?;
        Ok(Self {
            store: NatsEventStore::connect(js.clone(), events.clone()).await?,
            scope,
            events,
        })
    }

    async fn append(
        &self,
        aggregate: &str,
        id: &str,
        correlation: &str,
        causation: Option<&str>,
        count: usize,
    ) -> TestResult<Vec<RecordedEvent>> {
        let stream = StreamId::new(AggregateType::new(aggregate)?, AggregateId::new(id)?);
        let prior = u64::try_from(self.store.load(&stream).await?.len())?;
        let operation = OperationId::new(format!("{aggregate}-{id}-{prior}"))?;
        let commit = derive_commit_id(&stream, &operation);
        let events = (0..count)
            .map(|ordinal| {
                Ok(NewEvent::new(
                    derive_event_id(&commit, u32::try_from(ordinal)?),
                    format!("{aggregate}-changed"),
                    1,
                    serde_json::to_vec(&serde_json::json!({ "ordinal": ordinal }))?,
                )?)
            })
            .collect::<TestResult<Vec<_>>>()?;
        let mut batch = EventBatch::new(
            commit,
            operation,
            ContentFingerprint::digest(format!("{aggregate}-{id}-{prior}")),
            events,
        )?
        .with_correlation_id(CorrelationId::new(correlation)?);
        if let Some(causation) = causation {
            batch = batch.with_causation_id(CausationId::new(causation)?);
        }
        let expected = if prior == 0 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(prior))
        };
        Ok(self
            .store
            .append(&stream, expected, batch)
            .await?
            .into_events())
    }
}

fn now() -> TestResult<MessageTimestamp> {
    Ok(MessageTimestamp::from_unix_milliseconds(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)?)
}

async fn eventually<T, F, Fut>(mut probe: F) -> TestResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = TestResult<Option<T>>>,
{
    tokio::time::timeout(WAIT, async {
        loop {
            if let Some(value) = probe().await? {
                return TestResult::Ok(value);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?
}

async fn chain_with(
    reader: &NatsCorrelationIndexReader,
    correlation: &str,
    refs: usize,
) -> TestResult<CorrelationChain> {
    let correlation = CorrelationId::new(correlation)?;
    eventually(|| async {
        let chain = reader.chain(&correlation).await?;
        Ok((chain.refs.len() == refs).then_some(chain))
    })
    .await
}

async fn caught_up(reader: &NatsCorrelationIndexReader, contexts: &[&str]) -> TestResult {
    eventually(|| async {
        let lag = reader.freshness().await?.lag;
        Ok((lag.len() == contexts.len()
            && contexts
                .iter()
                .all(|context| lag.get(*context) == Some(&Some(0))))
        .then_some(()))
    })
    .await
}

async fn freshness_where(
    reader: &NatsCorrelationIndexReader,
    accept: impl Fn(&CorrelationIndexFreshness) -> bool + Send + Sync,
) -> TestResult<CorrelationIndexFreshness> {
    eventually(|| async {
        let freshness = reader.freshness().await?;
        Ok(accept(&freshness).then_some(freshness))
    })
    .await
}

fn states(
    freshness: &CorrelationIndexFreshness,
    context: &str,
) -> Vec<(CorrelationSourceKind, CorrelationSourceState, u64)> {
    freshness
        .sources
        .iter()
        .filter(|source| source.context == context)
        .map(|source| (source.kind, source.state, source.behind))
        .collect()
}

fn domain_event_ids(chain: &CorrelationChain) -> Vec<String> {
    chain
        .refs
        .iter()
        .filter_map(|reference| match reference {
            CorrelationRef::Domain(reference) => Some(reference.event_id.clone()),
            CorrelationRef::Integration(_) => None,
        })
        .collect()
}

#[tokio::test]
async fn committed_events_are_indexed_and_survive_a_projector_restart() -> TestResult {
    let fixture = Fixture::new().await?;
    let first = fixture
        .ordering
        .append("ordering/order", "O1", "R1", None, 1)
        .await?;
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;

    let chain = chain_with(&reader, "R1", 1).await?;
    let Some(CorrelationRef::Domain(indexed)) = chain.refs.first() else {
        panic!("expected a domain reference");
    };
    let recorded = first.first().ok_or("missing event")?;
    assert_eq!(indexed.context, "ordering");
    assert_eq!(indexed.aggregate_type, "ordering/order");
    assert_eq!(indexed.aggregate_id, "O1");
    assert_eq!(indexed.stream_version, 1);
    assert_eq!(indexed.event_id, recorded.event_id().as_str());
    assert_eq!(indexed.event_type, "ordering/order-changed");
    assert!(!chain.truncated);
    projector.stop().await?;

    let second = fixture
        .ordering
        .append(
            "ordering/order",
            "O1",
            "R1",
            Some(recorded.event_id().as_str()),
            1,
        )
        .await?;
    assert_eq!(chain_with(&reader, "R1", 1).await?, chain);
    let projector = fixture.start(&fixture.ordering).await?;
    let chain = chain_with(&reader, "R1", 2).await?;
    assert_eq!(
        domain_event_ids(&chain),
        [recorded, second.first().ok_or("missing event")?]
            .iter()
            .map(|event| event.event_id().as_str().to_owned())
            .collect::<Vec<_>>()
    );
    caught_up(&reader, &["ordering"]).await?;
    projector.stop().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn deleting_the_bucket_and_restarting_reproduces_identical_entries() -> TestResult {
    let fixture = Fixture::new().await?;
    let placed = fixture
        .ordering
        .append("ordering/order", "O1", "R1", None, 2)
        .await?;
    fixture
        .publish(
            &fixture.ordering,
            "order-placed",
            "R1",
            placed.last().ok_or("missing event")?,
        )
        .await?;
    fixture
        .ordering
        .append("ordering/order", "O2", "R2", None, 1)
        .await?;
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;
    caught_up(&reader, &["ordering"]).await?;
    let original = (
        chain_with(&reader, "R1", 3).await?,
        chain_with(&reader, "R2", 1).await?,
    );
    projector.stop().await?;

    fixture
        .js
        .delete_stream(fixture.index.stream_name())
        .await?;
    provision_read_model(&fixture.js, &fixture.index).await?;
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;
    caught_up(&reader, &["ordering"]).await?;
    let rebuilt = (
        chain_with(&reader, "R1", 3).await?,
        chain_with(&reader, "R2", 1).await?,
    );
    assert_eq!(rebuilt, original);
    projector.stop().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn references_beyond_the_cap_are_truncated_and_projection_continues() -> TestResult {
    let fixture = Fixture::new().await?;
    let cap = u64::try_from(MAX_CORRELATION_REFS)?;
    let mut appended = 0;
    while appended <= cap {
        let count = (cap + 1 - appended).min(100);
        fixture
            .ordering
            .append(
                "ordering/order",
                "BIG",
                "big",
                None,
                usize::try_from(count)?,
            )
            .await?;
        appended += count;
    }
    fixture
        .ordering
        .append("ordering/order", "NEXT", "after", None, 1)
        .await?;
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;

    chain_with(&reader, "after", 1).await?;
    let big = reader.chain(&CorrelationId::new("big")?).await?;
    assert_eq!(big.refs.len(), MAX_CORRELATION_REFS);
    assert!(big.truncated);
    caught_up(&reader, &["ordering"]).await?;
    assert!(!projector.task.is_finished());
    projector.stop().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn a_full_bucket_rejects_the_write_and_stops_the_projector_without_eviction() -> TestResult {
    let fixture = Fixture::with_index_limits(32 * 1024, 4 * 1024).await?;
    fixture
        .ordering
        .append("ordering/order", "O1", "first", None, 1)
        .await?;
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;
    let first = chain_with(&reader, "first", 1).await?;

    for number in 0..200 {
        fixture
            .ordering
            .append(
                "ordering/order",
                &format!("O{number}"),
                &format!("fill-{number}"),
                None,
                1,
            )
            .await?;
    }
    let error = tokio::time::timeout(WAIT, projector.task)
        .await??
        .expect_err("a full bucket must stop the projector");
    assert_eq!(error.kind(), ReadModelErrorKind::CapacityExhausted);
    assert_eq!(reader.chain(&CorrelationId::new("first")?).await?, first);
    let freshness = reader.freshness().await?;
    assert_eq!(freshness.lag.get("ordering"), Some(&None));
    let domain = freshness
        .sources
        .iter()
        .find(|source| source.kind == CorrelationSourceKind::Domain)
        .ok_or("missing domain source")?;
    assert_eq!(domain.state, CorrelationSourceState::Inactive);
    assert!(domain.behind > 0);
    fixture.cleanup().await
}

#[tokio::test]
async fn lag_is_reported_for_the_context_that_has_not_caught_up() -> TestResult {
    let fixture = Fixture::new().await?;
    let ordering = fixture.start(&fixture.ordering).await?;
    let billing = fixture.start(&fixture.billing).await?;
    let reader = fixture.reader().await?;
    caught_up(&reader, &["billing", "ordering"]).await?;
    billing.stop().await?;

    let placed = fixture
        .ordering
        .append("ordering/order", "O1", "R1", None, 1)
        .await?;
    fixture
        .billing
        .append(
            "billing/invoice",
            "I1",
            "R1",
            Some(placed.first().ok_or("missing event")?.event_id().as_str()),
            2,
        )
        .await?;
    chain_with(&reader, "R1", 1).await?;
    let freshness = freshness_where(&reader, |freshness| {
        freshness.lag.get("ordering") == Some(&Some(0))
    })
    .await?;
    assert_eq!(freshness.lag.get("billing"), Some(&None));
    assert_eq!(
        states(&freshness, "billing"),
        [
            (
                CorrelationSourceKind::Domain,
                CorrelationSourceState::Inactive,
                2
            ),
            (
                CorrelationSourceKind::Integration,
                CorrelationSourceState::Inactive,
                0
            ),
        ]
    );

    let billing = fixture.start(&fixture.billing).await?;
    chain_with(&reader, "R1", 3).await?;
    caught_up(&reader, &["billing", "ordering"]).await?;
    ordering.stop().await?;
    billing.stop().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn a_chain_crosses_contexts_through_its_integration_event_after_the_message_expires()
-> TestResult {
    let fixture = Fixture::new().await?;
    let ordering = fixture.start(&fixture.ordering).await?;
    let billing = fixture.start(&fixture.billing).await?;
    let placed = fixture
        .ordering
        .append("ordering/order", "O1", "R1", None, 1)
        .await?;
    let placed = placed.first().ok_or("missing event")?;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let (message_id, sequence) = fixture
        .publish(&fixture.ordering, "order-placed", "R1", placed)
        .await?;
    tokio::time::sleep(Duration::from_millis(5)).await;
    let invoiced = fixture
        .billing
        .append("billing/invoice", "I1", "R1", Some(message_id.as_str()), 1)
        .await?;

    let reader = fixture.reader().await?;
    caught_up(&reader, &["billing", "ordering"]).await?;
    let chain = chain_with(&reader, "R1", 3).await?;
    let [
        CorrelationRef::Domain(first),
        CorrelationRef::Integration(published),
        CorrelationRef::Domain(consumed),
    ] = chain.refs.as_slice()
    else {
        panic!("unexpected chain order: {chain:?}");
    };
    assert_eq!(first.event_id, placed.event_id().as_str());
    assert_eq!(published.context, "ordering");
    assert_eq!(published.event_type, "order-placed");
    assert_eq!(published.message_id, message_id.as_str());
    assert_eq!(
        published.causation_id.as_deref(),
        Some(first.event_id.as_str())
    );
    assert_eq!(published.sequence, sequence);
    assert_eq!(consumed.context, "billing");
    assert_eq!(
        consumed.event_id,
        invoiced.first().ok_or("missing event")?.event_id().as_str()
    );
    assert_eq!(consumed.causation_id.as_deref(), Some(message_id.as_str()));
    assert!(reader.integration_available(published).await?);

    fixture
        .js
        .get_stream(
            fixture
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?
        .delete_message(sequence)
        .await?;
    assert!(!reader.integration_available(published).await?);
    assert_eq!(reader.chain(&CorrelationId::new("R1")?).await?, chain);
    ordering.stop().await?;
    billing.stop().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn context_scoped_policies_and_misrouted_sources_are_rejected() -> TestResult {
    let fixture = Fixture::new().await?;
    let scoped = NatsReadModelConfig::new(&fixture.ordering.scope, "correlation-index")?;
    let error = NatsCorrelationIndexReader::connect(fixture.js.clone(), &scoped)
        .await
        .err()
        .ok_or("context-scoped policy accepted")?;
    assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
    let test = ApplicationName::new(fixture.index.application().as_str())?
        .test_bounded_context("ordering")?;
    let error = NatsCorrelationIndexProjector::connect(
        fixture.js.clone(),
        &fixture.index,
        NatsEventStoreConfig::for_bounded_context(&test)?,
        fixture.messaging.topology(),
    )
    .await
    .err()
    .ok_or("Test source accepted by a Normal index")?;
    assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
    fixture.cleanup().await
}

#[tokio::test]
async fn a_source_is_active_only_while_its_projector_consumer_exists() -> TestResult {
    let fixture = Fixture::new().await?;
    let events = fixture.ordering.events.stream_name().to_owned();
    let integration = fixture.integration_stream();
    let projector = fixture.start(&fixture.ordering).await?;
    let reader = fixture.reader().await?;
    let active = (CorrelationSourceState::Active, 0);
    let freshness = freshness_where(&reader, |freshness| {
        states(freshness, "ordering")
            .iter()
            .filter(|(_, state, behind)| (*state, *behind) == active)
            .count()
            == 2
    })
    .await?;
    assert_eq!(freshness.lag.get("ordering"), Some(&Some(0)));
    assert!(freshness.is_complete());
    let [domain] = fixture
        .projector_consumers(&events)
        .await?
        .try_into()
        .map_err(|names| format!("expected one domain consumer, got {names:?}"))?;
    assert!(domain.starts_with("correlation-index--domain--ordering--"));
    let [published] = fixture
        .projector_consumers(&integration)
        .await?
        .try_into()
        .map_err(|names| format!("expected one integration consumer, got {names:?}"))?;
    assert!(published.starts_with("correlation-index--integration--ordering--"));

    projector.stop().await?;
    assert!(fixture.projector_consumers(&events).await?.is_empty());
    assert!(fixture.projector_consumers(&integration).await?.is_empty());
    let freshness = reader.freshness().await?;
    assert_eq!(freshness.lag.get("ordering"), Some(&None));
    assert!(
        freshness
            .sources
            .iter()
            .all(|source| source.state == CorrelationSourceState::Inactive)
    );
    assert!(!freshness.is_complete());

    let projector = fixture.start(&fixture.ordering).await?;
    caught_up(&reader, &["ordering"]).await?;
    projector.task.abort();
    freshness_where(&reader, |freshness| {
        freshness.lag.get("ordering") == Some(&None)
    })
    .await?;
    assert!(fixture.projector_consumers(&events).await?.is_empty());
    fixture.cleanup().await
}

#[tokio::test]
async fn source_streams_that_could_lose_or_limit_consumers_are_rejected() -> TestResult {
    let fixture = Fixture::new().await?;
    let integration = fixture.integration_stream();
    fixture
        .recreate_stream(&integration, |config| {
            config.retention = RetentionPolicy::WorkQueue;
        })
        .await?;
    let error = NatsCorrelationIndexProjector::connect(
        fixture.js.clone(),
        &fixture.index,
        fixture.ordering.events.clone(),
        fixture.messaging.topology(),
    )
    .await
    .err()
    .ok_or("work-queue integration stream accepted")?;
    assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
    assert!(error.to_string().contains(&integration), "{error}");

    fixture
        .recreate_stream(fixture.billing.events.stream_name(), |config| {
            config.max_consumers = 8;
        })
        .await?;
    let error = NatsCorrelationIndexProjector::connect_domain_only(
        fixture.js.clone(),
        &fixture.index,
        fixture.billing.events.clone(),
    )
    .await
    .err()
    .ok_or("consumer-limited event store accepted")?;
    assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
    fixture.cleanup().await
}

#[tokio::test]
async fn a_domain_only_projector_refuses_a_context_indexed_with_integration_events() -> TestResult {
    let fixture = Fixture::new().await?;
    let reader = fixture.reader().await?;
    let projector = fixture.start(&fixture.ordering).await?;
    freshness_where(&reader, |freshness| {
        states(freshness, "ordering").len() == 2
    })
    .await?;
    projector.stop().await?;
    let error = NatsCorrelationIndexProjector::connect_domain_only(
        fixture.js.clone(),
        &fixture.index,
        fixture.ordering.events.clone(),
    )
    .await
    .err()
    .ok_or("domain-only projector accepted an integration-indexed context")?;
    assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
    assert!(error.to_string().contains("rebuild"), "{error}");

    fixture
        .billing
        .append("billing/invoice", "I1", "R1", None, 1)
        .await?;
    let billing = Running::spawn(
        NatsCorrelationIndexProjector::connect_domain_only(
            fixture.js.clone(),
            &fixture.index,
            fixture.billing.events.clone(),
        )
        .await?,
    );
    chain_with(&reader, "R1", 1).await?;
    let freshness = freshness_where(&reader, |freshness| {
        freshness.lag.get("billing") == Some(&Some(0))
    })
    .await?;
    assert_eq!(
        states(&freshness, "billing"),
        [(
            CorrelationSourceKind::Domain,
            CorrelationSourceState::Active,
            0
        )]
    );
    billing.stop().await?;
    fixture.cleanup().await
}
