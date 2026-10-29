use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream;
use futures_util::TryStreamExt;
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, CorrelationChain, CorrelationRead,
    CorrelationSourceKind, CorrelationSourceSnapshot, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{
    ApplicationName, CausationId, CorrelationId, EnvelopeContext, IntegrationEventEnvelope,
    MessageId, MessageTimestamp, OutboundMessage, SchemaVersion, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, MessagingTopology, NatsConnection, NatsConnectionConfig,
    NatsCorrelationHistory, NatsEventStore, NatsEventStoreConfig, connect, provision_event_store,
    provision_stream,
};
use serde_json::{Value, json};

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_APPLICATION: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub connection: NatsConnection,
    pub topology: MessagingTopology,
    pub sales: NatsEventStore,
    pub shipping: NatsEventStore,
}

impl Fixture {
    pub async fn new() -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sequence = NEXT_APPLICATION.fetch_add(1, Ordering::Relaxed);
        let application = ApplicationName::new(format!(
            "history-{:x}-{nanos:x}-{sequence:x}",
            std::process::id()
        ))?;
        Self::in_scope(application, TrafficScope::Normal).await
    }

    pub async fn in_scope(application: ApplicationName, scope: TrafficScope) -> TestResult<Self> {
        let connection = new_connection().await?;
        let messaging = ApplicationMessagingConfig::new_in_scope(&application, scope)?
            .with_max_bytes(32 * 1024 * 1024)?;
        provision_stream(connection.jetstream(), messaging.integration_events()).await?;
        let sales = domain_store(&connection, &application, scope, "sales").await?;
        let shipping = domain_store(&connection, &application, scope, "shipping").await?;
        Ok(Self {
            connection,
            topology: messaging.topology().clone(),
            sales,
            shipping,
        })
    }

    pub const fn js(&self) -> &jetstream::Context {
        self.connection.jetstream()
    }

    pub fn configs(&self) -> Vec<NatsEventStoreConfig> {
        vec![self.sales.config().clone(), self.shipping.config().clone()]
    }

    pub async fn reader(&self) -> TestResult<NatsCorrelationHistory> {
        Ok(
            NatsCorrelationHistory::connect(self.js().clone(), self.configs(), &self.topology)
                .await?,
        )
    }

    pub fn source_names(&self) -> [&str; 3] {
        [
            self.sales.config().stream_name(),
            self.shipping.config().stream_name(),
            self.topology.integration_event_stream().as_str(),
        ]
    }

    pub async fn snapshot(&self) -> TestResult<Vec<CorrelationSourceSnapshot>> {
        let mut sources = Vec::new();
        for config in self.configs() {
            sources.push(CorrelationSourceSnapshot {
                context: Some(config.bounded_context().as_str().to_owned()),
                kind: CorrelationSourceKind::Domain,
                cutoff_sequence: self
                    .js()
                    .get_stream(config.stream_name())
                    .await?
                    .cached_info()
                    .state
                    .last_sequence
                    .to_string(),
                stream: config.stream_name().to_owned(),
            });
        }
        let stream = self
            .js()
            .get_stream(self.topology.integration_event_stream().as_str())
            .await?;
        sources.push(CorrelationSourceSnapshot {
            context: None,
            kind: CorrelationSourceKind::Integration,
            stream: self.topology.integration_event_stream().as_str().to_owned(),
            cutoff_sequence: stream.cached_info().state.last_sequence.to_string(),
        });
        sources.sort_by(|left, right| left.stream.cmp(&right.stream));
        Ok(sources)
    }

    pub async fn publish(
        &self,
        producer: &str,
        message_id: &str,
        correlation: &str,
        causation: Option<&str>,
    ) -> TestResult<rostfrei_core::IntegrationEventRef> {
        let address = self
            .topology
            .application()
            .bounded_context_in_scope(self.topology.traffic_scope(), producer)?
            .integration_event_address("order-updated")?;
        let envelope = integration_envelope(message_id, correlation, causation)?;
        let acknowledgement = self
            .connection
            .publisher(self.topology.clone())
            .publish_integration_event_with_ack(
                OutboundMessage::json(address.clone(), envelope.message_id().clone(), &envelope)?,
                Duration::from_secs(5),
            )
            .await?;
        let stored = self
            .js()
            .get_stream(self.topology.integration_event_stream().as_str())
            .await?
            .get_raw_message(acknowledgement.sequence())
            .await?;
        Ok(rostfrei_core::IntegrationEventRef {
            context: producer.to_owned(),
            subject: stored.subject.to_string(),
            message_id: message_id.to_owned(),
            event_type: "order-updated".to_owned(),
            schema_version: 7,
            causation_id: causation.map(str::to_owned),
            occurred_at: envelope.occurred_at(),
        })
    }

    pub async fn cleanup(self) -> TestResult {
        let existing = stream_names(self.js()).await?;
        for name in self.source_names() {
            if existing.contains(name) {
                self.js().delete_stream(name).await?;
            }
        }
        self.connection.drain().await?;
        Ok(())
    }
}

pub async fn new_connection() -> TestResult<NatsConnection> {
    Ok(connect(&NatsConnectionConfig::new(
        "correlation-history-test",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?)
}

async fn domain_store(
    connection: &NatsConnection,
    application: &ApplicationName,
    scope: TrafficScope,
    name: &str,
) -> TestResult<NatsEventStore> {
    let context = application.bounded_context_in_scope(scope, name)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(32 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(connection.jetstream(), &config).await?;
    Ok(NatsEventStore::connect(connection.jetstream().clone(), config).await?)
}

pub async fn append_domain(
    store: &NatsEventStore,
    aggregate_id: &str,
    correlation: &str,
    causation: &str,
    count: usize,
) -> TestResult<Vec<RecordedEvent>> {
    let stream = StreamId::new(
        AggregateType::new("Order")?,
        AggregateId::new(aggregate_id)?,
    );
    let mut recorded = Vec::new();
    while recorded.len() < count {
        let operation = OperationId::new(format!("{aggregate_id}-{}", recorded.len()))?;
        let commit = derive_commit_id(&stream, &operation);
        let remaining = count
            .checked_sub(recorded.len())
            .ok_or("event count overflow")?;
        let events = (0..remaining.min(100))
            .map(|ordinal| {
                Ok(NewEvent::new(
                    derive_event_id(&commit, u32::try_from(ordinal)?),
                    "order-changed",
                    1,
                    // The reader needs no application payload codec, even for non-JSON facts.
                    b"opaque-domain-payload".to_vec(),
                )?)
            })
            .collect::<TestResult<Vec<_>>>()?;
        let batch = EventBatch::new(
            commit,
            operation,
            ContentFingerprint::digest(aggregate_id),
            events,
        )?
        .with_correlation_id(CorrelationId::new(correlation)?)
        .with_causation_id(CausationId::new(causation)?);
        let version = if recorded.is_empty() {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(u64::try_from(recorded.len())?))
        };
        recorded.extend(store.append(&stream, version, batch).await?.into_events());
    }
    Ok(recorded)
}

pub fn integration_envelope(
    id: &str,
    correlation: &str,
    causation: Option<&str>,
) -> TestResult<IntegrationEventEnvelope<Value>> {
    Ok(IntegrationEventEnvelope::new(
        EnvelopeContext::new(
            MessageId::new(id)?,
            SchemaVersion::new(7)?,
            CorrelationId::new(correlation)?,
            causation.map(CausationId::new).transpose()?,
        ),
        MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?,
        json!({"private_business_payload": "not part of a correlation reference"}),
    )?)
}

pub fn chain<'a>(read: &'a CorrelationRead, id: &str) -> TestResult<&'a CorrelationChain> {
    let mut matches = read
        .chains
        .iter()
        .filter(|chain| chain.correlation_id == id);
    let found = matches.next().ok_or("requested correlation was omitted")?;
    if matches.next().is_some() {
        return Err("requested correlation appeared more than once".into());
    }
    Ok(found)
}

pub async fn stream_names(context: &jetstream::Context) -> TestResult<BTreeSet<String>> {
    Ok(context.stream_names().try_collect().await?)
}

pub async fn wait_for_consumer_count(fixture: &Fixture, expected: usize) -> TestResult {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let mut matches = true;
            for name in fixture.source_names() {
                matches &= fixture
                    .js()
                    .get_stream(name)
                    .await?
                    .cached_info()
                    .state
                    .consumer_count
                    == expected;
            }
            if matches {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    Ok(())
}
