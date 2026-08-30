use std::{
    error::Error,
    io, process,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::{
    HeaderMap,
    jetstream::{
        self,
        consumer::{self, AckPolicy, DeliverPolicy},
        stream::{Config, RetentionPolicy, StorageType},
    },
};
use async_trait::async_trait;
use futures_util::StreamExt;
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    CORRELATION_ID_HEADER, CorrelatedMessage, CorrelatedMessageFamily, CorrelatedMessageHandler,
    NatsCorrelationObserver,
};
use tokio::{sync::mpsc, time::Instant};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(5);
const ABSENCE_TIMEOUT: Duration = Duration::from_millis(500);

struct ChannelHandler {
    sender: mpsc::UnboundedSender<CorrelatedMessage>,
}

#[async_trait]
impl CorrelatedMessageHandler for ChannelHandler {
    async fn handle(&self, message: CorrelatedMessage) {
        let _ = self.sender.send(message);
    }
}

#[tokio::test]
#[ignore = "requires a real NATS server with JetStream and ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn observer_reads_new_persisted_messages_without_advancing_worker_consumers() -> TestResult {
    let nats_url = std::env::var("ROSTFREI_NATS_URL")?;
    let suffix = unique_suffix()?;
    let application = ApplicationName::new(format!("correlation-{suffix}"))?;
    let domain_stream = format!("CORRELATION_{suffix}_DOMAIN").to_ascii_uppercase();
    let other_domain_stream = format!("CORRELATION_{suffix}_OTHER_DOMAIN").to_ascii_uppercase();
    let late_domain_stream = format!("CORRELATION_{suffix}_LATE_DOMAIN").to_ascii_uppercase();
    let integration_stream = format!("CORRELATION_{suffix}_INTEGRATION").to_ascii_uppercase();
    let domain_subject = format!("{}.domain.test.aggregate.one", application.as_str());
    let other_domain_subject = format!("{}.domain.other.aggregate.two", application.as_str());
    let late_domain_subject = format!("{}.domain.late.aggregate.three", application.as_str());
    let integration_subject = format!("{}.integration.test-event", application.as_str());
    let client = async_nats::connect(nats_url).await?;
    let context = jetstream::new(client.clone());
    let domain_config = stream_config(
        &domain_stream,
        format!("{}.domain.test.>", application.as_str()),
    );
    let other_domain_config = stream_config(
        &other_domain_stream,
        format!("{}.domain.other.>", application.as_str()),
    );
    let integration_config = stream_config(
        &integration_stream,
        format!("{}.integration.>", application.as_str()),
    );
    let domain = context.create_stream(domain_config.clone()).await?;
    context.create_stream(other_domain_config).await?;
    let integration = context.create_stream(integration_config.clone()).await?;

    publish_correlated(
        &context,
        &domain_subject,
        "old-domain",
        "old-domain-correlation",
        b"old domain",
    )
    .await?;
    publish_correlated(
        &context,
        &integration_subject,
        "old-integration",
        "old-integration-correlation",
        b"old integration",
    )
    .await?;

    let worker_name = format!("correlation-worker-{suffix}");
    let worker: consumer::PullConsumer = integration
        .create_consumer(consumer::pull::Config {
            durable_name: Some(worker_name.clone()),
            name: Some(worker_name),
            deliver_policy: DeliverPolicy::All,
            ack_policy: AckPolicy::Explicit,
            filter_subject: integration_subject.clone(),
            ..Default::default()
        })
        .await?;
    let mut worker_info = worker.clone();

    let subscription = NatsCorrelationObserver::new(client.clone(), application.clone())
        .subscribe()
        .await?;
    let (sender, mut observations) = mpsc::unbounded_channel();
    let observer_task = tokio::spawn(subscription.run(Arc::new(ChannelHandler { sender })));

    ensure(
        tokio::time::timeout(ABSENCE_TIMEOUT, observations.recv())
            .await
            .is_err(),
        "observer replayed messages stored before subscription",
    )?;
    let mut observer_consumers = domain.consumer_names();
    let observer_consumer = tokio::time::timeout(OBSERVATION_TIMEOUT, observer_consumers.next())
        .await?
        .ok_or_else(|| io::Error::other("observer consumer was not created"))??;
    domain.delete_consumer(&observer_consumer).await?;

    publish_correlated(
        &context,
        &domain_subject,
        "accepted-domain",
        "accepted-domain-correlation",
        b"domain",
    )
    .await?;
    publish_correlated(
        &context,
        &integration_subject,
        "accepted-integration",
        "accepted-integration-correlation",
        b"integration",
    )
    .await?;
    publish_correlated(
        &context,
        &other_domain_subject,
        "accepted-other-domain",
        "accepted-other-domain-correlation",
        b"other domain",
    )
    .await?;

    let first = receive_observation(&mut observations).await?;
    let second = receive_observation(&mut observations).await?;
    let third = receive_observation(&mut observations).await?;
    let observed = [first, second, third];
    ensure(
        observed.iter().any(|message| {
            message.family() == CorrelatedMessageFamily::DomainEvent
                && message.correlation_id().as_str() == "accepted-domain-correlation"
                && message
                    .message_id()
                    .is_some_and(|id| id.as_str() == "accepted-domain")
                && has_test_payload(message, "domain")
        }),
        "observer missed the committed domain event",
    )?;
    ensure(
        observed.iter().any(|message| {
            message.family() == CorrelatedMessageFamily::DomainEvent
                && message.correlation_id().as_str() == "accepted-other-domain-correlation"
                && has_test_payload(message, "other domain")
        }),
        "observer missed a domain event from another bounded-context stream",
    )?;
    ensure(
        observed.iter().any(|message| {
            message.family() == CorrelatedMessageFamily::IntegrationEvent
                && message.correlation_id().as_str() == "accepted-integration-correlation"
                && message
                    .message_id()
                    .is_some_and(|id| id.as_str() == "accepted-integration")
                && has_test_payload(message, "integration")
        }),
        "observer missed the committed integration event",
    )?;

    context
        .create_stream(stream_config(
            &late_domain_stream,
            format!("{}.domain.late.>", application.as_str()),
        ))
        .await?;
    publish_correlated(
        &context,
        &late_domain_subject,
        "late-domain",
        "late-domain-correlation",
        b"late domain",
    )
    .await?;
    let late = receive_observation(&mut observations).await?;
    ensure(
        late.family() == CorrelatedMessageFamily::DomainEvent
            && late.correlation_id().as_str() == "late-domain-correlation"
            && has_test_payload(&late, "late domain"),
        "observer did not discover a stream created after startup",
    )?;

    let duplicate = publish_correlated(
        &context,
        &integration_subject,
        "accepted-integration",
        "duplicate-attempt-correlation",
        b"duplicate attempt",
    )
    .await?;
    ensure(duplicate, "JetStream did not deduplicate the test publish")?;

    let mut rejected_headers = correlated_headers("rejected-domain", "rejected-correlation");
    rejected_headers.insert("Content-Type", "application/octet-stream");
    let rejected = context
        .publish_with_headers(
            domain_subject.clone(),
            rejected_headers,
            vec![0_u8; 2 * 1024].into(),
        )
        .await?
        .await;
    ensure(
        rejected.is_err(),
        "JetStream accepted an oversized test publish",
    )?;

    let uncorrelated_ack = context
        .publish(domain_subject.clone(), b"uncorrelated".as_slice().into())
        .await?
        .await?;
    ensure(
        !uncorrelated_ack.duplicate,
        "uncorrelated test publish was unexpectedly deduplicated",
    )?;
    ensure(
        tokio::time::timeout(ABSENCE_TIMEOUT, observations.recv())
            .await
            .is_err(),
        "observer reported a rejected, duplicate, or uncorrelated publish",
    )?;

    let mut worker_messages = worker.messages().await?;
    let old_worker_message = receive_worker_message(&mut worker_messages).await?;
    ensure(
        has_raw_test_payload(&old_worker_message.payload, "old integration"),
        "worker did not retain its pre-observer message",
    )?;
    old_worker_message.ack().await?;
    let current_worker_message = receive_worker_message(&mut worker_messages).await?;
    ensure(
        has_raw_test_payload(&current_worker_message.payload, "integration"),
        "worker did not independently receive the observed integration event",
    )?;
    wait_for_ack_pending(&mut worker_info, 1).await?;

    context.delete_stream(&domain_stream).await?;
    context.create_stream(domain_config).await?;
    publish_correlated(
        &context,
        &domain_subject,
        "replacement-domain",
        "replacement-correlation",
        b"replacement",
    )
    .await?;
    let replacement = receive_observation(&mut observations).await?;
    ensure(
        replacement.family() == CorrelatedMessageFamily::DomainEvent
            && replacement.correlation_id().as_str() == "replacement-correlation"
            && has_test_payload(&replacement, "replacement"),
        "observer did not replay the new stream generation",
    )?;
    ensure(
        tokio::time::timeout(ABSENCE_TIMEOUT, observations.recv())
            .await
            .is_err(),
        "observer duplicated a message from the replacement stream",
    )?;

    current_worker_message.ack().await?;
    observer_task.abort();
    let _ = observer_task.await;
    context.delete_stream(&domain_stream).await?;
    context.delete_stream(&other_domain_stream).await?;
    context.delete_stream(&late_domain_stream).await?;
    context.delete_stream(&integration_stream).await?;
    client.drain().await?;
    Ok(())
}

fn stream_config(name: &str, subject: String) -> Config {
    Config {
        name: name.to_owned(),
        subjects: vec![subject],
        retention: RetentionPolicy::Limits,
        storage: StorageType::Memory,
        max_message_size: 1024,
        duplicate_window: Duration::from_secs(60),
        ..Default::default()
    }
}

async fn publish_correlated(
    context: &jetstream::Context,
    subject: &str,
    message_id: &str,
    correlation_id: &str,
    payload: &[u8],
) -> TestResult<bool> {
    let test_payload = String::from_utf8(payload.to_vec())?;
    let payload = if subject.contains(".domain.") {
        serde_json::json!({
            "event": {
                "eventId": message_id,
                "correlationId": correlation_id
            },
            "testPayload": test_payload
        })
    } else {
        serde_json::json!({
            "message_id": message_id,
            "correlation_id": correlation_id,
            "testPayload": test_payload
        })
    };
    let acknowledgement = context
        .publish_with_headers(
            subject.to_owned(),
            correlated_headers(message_id, correlation_id),
            serde_json::to_vec(&payload)?.into(),
        )
        .await?
        .await?;
    Ok(acknowledgement.duplicate)
}

fn correlated_headers(message_id: &str, correlation_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Nats-Msg-Id", message_id);
    headers.insert(CORRELATION_ID_HEADER, correlation_id);
    headers.insert("Content-Type", "application/json");
    headers
}

fn has_test_payload(message: &CorrelatedMessage, expected: &str) -> bool {
    has_raw_test_payload(message.payload(), expected)
}

fn has_raw_test_payload(payload: &[u8], expected: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(payload)
        .ok()
        .and_then(|payload| {
            payload
                .get("testPayload")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == expected)
        })
        .unwrap_or(false)
}

async fn receive_observation(
    observations: &mut mpsc::UnboundedReceiver<CorrelatedMessage>,
) -> TestResult<CorrelatedMessage> {
    tokio::time::timeout(OBSERVATION_TIMEOUT, observations.recv())
        .await?
        .ok_or_else(|| io::Error::other("correlation observer stopped").into())
}

async fn receive_worker_message(
    messages: &mut consumer::pull::Stream,
) -> TestResult<jetstream::Message> {
    tokio::time::timeout(OBSERVATION_TIMEOUT, messages.next())
        .await?
        .ok_or_else(|| io::Error::other("worker consumer stopped"))?
        .map_err(Into::into)
}

async fn wait_for_ack_pending(
    consumer: &mut consumer::PullConsumer,
    expected: usize,
) -> TestResult {
    let deadline = Instant::now()
        .checked_add(OBSERVATION_TIMEOUT)
        .ok_or_else(|| io::Error::other("worker acknowledgement deadline overflowed"))?;
    loop {
        if consumer.info().await?.num_ack_pending == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "observer advanced the application worker consumer acknowledgement state",
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn unique_suffix() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("{:x}-{nanos:x}", process::id()))
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
