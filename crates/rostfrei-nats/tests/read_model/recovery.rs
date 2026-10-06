use std::{
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use async_nats::jetstream::{self, consumer::PullConsumer};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rostfrei::{
    ApplicationName, JsonReadModelCodec, ReadModelEntry, ReadModelError, ReadModelErrorKind,
    ReadModelKey, ReadModelRevision, ReadModelStore,
};
use rostfrei_messaging_core::{
    CallerMetadata, ConsumerConfig, CorrelationId, EnvelopeContext, IntegrationEventAddress,
    IntegrationEventEnvelope, MessageConsumerFactory, MessageId, MessageTimestamp, OutboundMessage,
    QuarantineFailureKind, SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConsumerFactory, NatsPublishAck, NatsPublisher,
    NatsReadModelConfig, NatsReadModelStore, QuarantineRecord, provision_application_messaging,
    provision_durable_consumer, provision_read_model,
};

use super::application::{BillingChanged, Entitlement, Entitlements, ExampleResult};

type Store = NatsReadModelStore<Entitlement, JsonReadModelCodec<Entitlement>>;
const WAIT: Duration = Duration::from_secs(10);

/// Inject writes failing before commit; all successful operations use real KV.
struct FailingWrites {
    inner: Arc<Store>,
    enabled: AtomicBool,
    failures: AtomicU32,
    kind: ReadModelErrorKind,
}

impl FailingWrites {
    fn check(&self) -> Result<(), ReadModelError> {
        if self.enabled.load(Ordering::SeqCst) {
            self.failures.fetch_add(1, Ordering::SeqCst);
            return Err(ReadModelError::new(
                self.kind,
                "injected pre-commit storage failure",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl ReadModelStore<Entitlement> for FailingWrites {
    async fn read(
        &self,
        key: &ReadModelKey,
    ) -> Result<Option<ReadModelEntry<Entitlement>>, ReadModelError> {
        self.inner.read(key).await
    }

    async fn create(
        &self,
        key: &ReadModelKey,
        value: &Entitlement,
    ) -> Result<ReadModelRevision, ReadModelError> {
        self.check()?;
        self.inner.create(key, value).await
    }

    async fn update(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
        value: &Entitlement,
    ) -> Result<ReadModelRevision, ReadModelError> {
        self.check()?;
        self.inner.update(key, revision, value).await
    }

    async fn delete(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
    ) -> Result<(), ReadModelError> {
        self.check()?;
        self.inner.delete(key, revision).await
    }
}

struct Recovery {
    context: jetstream::Context,
    messaging: ApplicationMessagingConfig,
    consumer: ConsumerConfig<IntegrationEventAddress>,
    model: NatsReadModelConfig,
    storage: Arc<FailingWrites>,
    handler: Arc<Entitlements>,
}

impl Recovery {
    async fn new(label: &str, kind: ReadModelErrorKind) -> ExampleResult<Self> {
        let client = async_nats::connect(rostfrei_testing::integration::nats_url()?).await?;
        let context = jetstream::new(client);
        let application = ApplicationName::new(format!(
            "read-model-recovery-{}-{label}",
            std::process::id()
        ))?;
        let scope = application.bounded_context("access")?;
        let messaging =
            ApplicationMessagingConfig::new(&application)?.with_max_bytes(4 * 1024 * 1024)?;
        provision_application_messaging(&context, &messaging).await?;
        let consumer = ConsumerConfig::new(
            scope.consumer_name("entitlements", 1)?,
            scope.durable_name("entitlements", 1)?,
            scope.integration_event_address("billing-changed")?,
            Duration::from_secs(4),
            Duration::from_secs(2),
            1,
            2,
        )?;
        provision_durable_consumer(&context, messaging.topology(), &consumer).await?;
        let model = NatsReadModelConfig::new(&scope, "entitlements")?;
        provision_read_model(&context, &model).await?;
        let inner = Arc::new(
            Store::connect(
                context.clone(),
                model.clone(),
                JsonReadModelCodec::new(NonZeroU32::MIN),
            )
            .await?,
        );
        inner
            .create(
                &ReadModelKey::new("org-1")?,
                &Entitlement {
                    members: 3,
                    demo: false,
                    paid: true,
                    organization_version: 12,
                    billing_version: 7,
                },
            )
            .await?;
        let storage = Arc::new(FailingWrites {
            inner,
            kind,
            enabled: AtomicBool::new(true),
            failures: AtomicU32::new(0),
        });
        let handler = Arc::new(Entitlements::new(storage.clone())?);
        Ok(Self {
            context,
            messaging,
            consumer,
            model,
            storage,
            handler,
        })
    }

    async fn run(
        &self,
        expected_failures: u32,
        expected_kind: QuarantineFailureKind,
    ) -> ExampleResult {
        let factory =
            NatsConsumerFactory::new(self.context.clone(), self.messaging.topology().clone());
        let consumer = factory.create(self.consumer.clone())?;
        // Cancels the worker before cleanup even if the walkthrough fails.
        let result = tokio::select! {
            result = consumer.run(self.handler.clone()) => {
                result?;
                Err("integration consumer exited before recovery completed".into())
            }
            result = self.walkthrough(expected_failures, expected_kind) => result,
        };
        let cleanup = self.cleanup().await;
        result?;
        cleanup
    }

    async fn snapshot(&self) -> ExampleResult<ReadModelEntry<Entitlement>> {
        self.storage
            .inner
            .read(&ReadModelKey::new("org-1")?)
            .await?
            .ok_or_else(|| "snapshot missing".into())
    }

    async fn publish(
        &self,
        envelope: &IntegrationEventEnvelope<BillingChanged>,
        metadata: CallerMetadata,
    ) -> ExampleResult<NatsPublishAck> {
        let message = OutboundMessage::json(
            self.consumer.address().clone(),
            envelope.message_id().clone(),
            envelope,
        )?
        .with_metadata(metadata)
        .with_correlation_id(envelope.correlation_id().clone());
        Ok(
            NatsPublisher::new(self.context.clone(), self.messaging.topology().clone())
                .publish_integration_event_with_ack(message, WAIT)
                .await?,
        )
    }

    async fn wait_settled(&self, sequence: u64) -> ExampleResult {
        let stream = self
            .context
            .get_stream(
                self.messaging
                    .topology()
                    .integration_event_stream()
                    .as_str(),
            )
            .await?;
        let inspector: PullConsumer = stream
            .get_consumer(self.consumer.durable_name().as_str())
            .await?;
        tokio::time::timeout(WAIT, async {
            loop {
                let info = inspector.get_info().await?;
                if info.ack_floor.stream_sequence >= sequence
                    && info.num_ack_pending == 0
                    && info.num_pending == 0
                {
                    return ExampleResult::Ok(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        Ok(())
    }

    async fn quarantine_record(&self) -> ExampleResult<QuarantineRecord> {
        let stream = self
            .context
            .get_stream(self.messaging.topology().quarantine_stream().as_str())
            .await?;
        assert_eq!(stream.cached_info().state.messages, 1);
        let record = stream
            .get_raw_message(stream.cached_info().state.last_sequence)
            .await?;
        Ok(serde_json::from_slice(&record.payload)?)
    }

    async fn walkthrough(
        &self,
        expected_failures: u32,
        expected_kind: QuarantineFailureKind,
    ) -> ExampleResult {
        let envelope = IntegrationEventEnvelope::new(
            EnvelopeContext::new(
                MessageId::new("billing-8")?,
                SchemaVersion::new(1)?,
                CorrelationId::new("billing-recovery")?,
                None,
            ),
            MessageTimestamp::from_unix_milliseconds(1)?,
            BillingChanged {
                organization_id: "org-1".to_owned(),
                source_version: 8,
                paid: false,
            },
        )?;
        let before = self.snapshot().await?;
        let original = self.publish(&envelope, CallerMetadata::default()).await?;
        assert!(!original.duplicate());
        self.wait_settled(original.sequence()).await?;
        let record = self.quarantine_record().await?;
        assert_eq!(record.failure_kind(), Some(expected_kind));
        assert_eq!(record.attempt(), expected_failures);
        assert_eq!(
            self.storage.failures.load(Ordering::SeqCst),
            expected_failures
        );
        assert_eq!(record.source_sequence(), original.sequence());
        assert_eq!(record.message_id(), envelope.message_id().as_str());
        assert_eq!(record.address(), self.consumer.address().as_str());
        assert!(!record.payload_truncated());
        if expected_kind == QuarantineFailureKind::HandlerFailure {
            assert!(record.reason().contains("snapshot not materialized"));
        }
        assert_eq!(self.snapshot().await?, before); // Terminal consumer progress did not materialize v8.

        self.storage.enabled.store(false, Ordering::SeqCst);
        // Even republishing with the original ID is suppressed by the duplicate
        // window. Repair alone cannot recover the already-terminated delivery.
        let duplicate = self.publish(&envelope, CallerMetadata::default()).await?;
        assert!(duplicate.duplicate());
        assert_eq!(duplicate.sequence(), original.sequence());
        assert_eq!(self.snapshot().await?, before);

        let payload = BASE64.decode(record.payload_base64())?;
        assert_eq!(payload, serde_json::to_vec(&envelope)?);
        let recovered = recovery_envelope(&payload, "billing-8-recovery")?;
        let mut metadata = record.metadata().clone();
        metadata.insert("x-redrive-of", record.message_id())?;
        let published = self.publish(&recovered, metadata.clone()).await?;
        assert!(!published.duplicate());
        self.wait_settled(published.sequence()).await?;
        let after = self.snapshot().await?;
        let mut expected = before.value;
        expected.paid = false;
        expected.billing_version = 8;
        assert_eq!(after.value, expected); // Other handlers' fields/checkpoints survive recovery.
        assert_ne!(after.revision, before.revision);

        // A second recovery publication is delivered, but the source checkpoint
        // makes it a no-op without another snapshot write.
        let replayed = recovery_envelope(&payload, "billing-8-recovery-again")?;
        let published = self.publish(&replayed, metadata).await?;
        assert!(!published.duplicate());
        self.wait_settled(published.sequence()).await?;
        assert_eq!(self.snapshot().await?, after);
        assert_eq!(
            self.quarantine_record().await?.source_sequence(),
            original.sequence()
        );
        Ok(())
    }

    async fn cleanup(&self) -> ExampleResult {
        for stream in self.messaging.streams() {
            self.context.delete_stream(stream.name().as_str()).await?;
        }
        self.context.delete_stream(self.model.stream_name()).await?;
        Ok(())
    }
}

fn recovery_envelope(
    payload: &[u8],
    id: &str,
) -> ExampleResult<IntegrationEventEnvelope<BillingChanged>> {
    let original: IntegrationEventEnvelope<BillingChanged> = serde_json::from_slice(payload)?;
    let context = EnvelopeContext::new(
        MessageId::new(id)?,
        original.schema_version(),
        original.correlation_id().clone(),
        Some(original.message_id().into()),
    );
    Ok(IntegrationEventEnvelope::new(
        context,
        original.occurred_at(),
        original.into_payload(),
    )?)
}

#[tokio::test]
async fn capacity_quarantine_recovers_only_after_explicit_republication() -> ExampleResult {
    Recovery::new("capacity", ReadModelErrorKind::CapacityExhausted)
        .await?
        .run(1, QuarantineFailureKind::HandlerFailure)
        .await
}

#[tokio::test]
async fn exhausted_retries_recover_only_after_explicit_republication() -> ExampleResult {
    Recovery::new("unavailable", ReadModelErrorKind::Unavailable)
        .await?
        .run(2, QuarantineFailureKind::DeliveryAttemptsExhausted)
        .await
}
