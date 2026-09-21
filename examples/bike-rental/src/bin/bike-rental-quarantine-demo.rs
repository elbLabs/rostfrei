//! A self-checking quarantine walkthrough using real `JetStream` deliveries.
use std::{
    env,
    error::Error,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rostfrei_messaging_core::{
    ApplicationName, CallerMetadata, CommandAddress, ConsumerConfig, DeliveryDisposition,
    MessageConsumerFactory, MessageDelivery, MessageHandler, MessageId, OutboundMessage,
    QuarantineFailureKind, QuarantineReason, RetryDelay, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsConnectionConfig, QuarantineRecord,
    ServerVersion, connect, provision_application_messaging, provision_durable_consumer,
};
use serde::Deserialize;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

type DemoResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const WAIT: Duration = Duration::from_secs(15);
const PAYLOAD: &[u8] = br#"{"fleet_id":"city-fleet","bicycle_id":"bike-42"}"#;
const INVALID_PAYLOAD: &[u8] = br#"{"fleet_id":"city-fleet"}"#;
const INVALID_REASON: &str = "rental notification requires fleet_id and bicycle_id";

#[derive(Deserialize)]
struct RentalNotification {
    fleet_id: String,
    bicycle_id: String,
}

// The only fake is the external notification service. Publishing, retrying,
// quarantining, termination, and acknowledgement all use real NATS adapters.
struct NotificationHandler {
    service_available: AtomicBool,
    calls: AtomicU32,
    successes: AtomicU32,
    retry_delay: RetryDelay,
    invalid_payload: QuarantineReason,
}

impl NotificationHandler {
    fn new() -> DemoResult<Self> {
        Ok(Self {
            service_available: AtomicBool::new(false),
            calls: AtomicU32::new(0),
            successes: AtomicU32::new(0),
            retry_delay: RetryDelay::new(Duration::from_millis(250))?,
            invalid_payload: QuarantineReason::new(INVALID_REASON)?,
        })
    }
}

#[async_trait]
impl MessageHandler<CommandAddress> for NotificationHandler {
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        self.calls.fetch_add(1, Ordering::SeqCst);
        println!(
            "  delivery {}: attempt {}",
            delivery.message_id(),
            delivery.attempt()
        );
        let Ok(notification) = serde_json::from_slice::<RentalNotification>(delivery.payload())
        else {
            println!("  -> Quarantine: {INVALID_REASON}");
            return DeliveryDisposition::Quarantine(self.invalid_payload.clone());
        };
        if !self.service_available.load(Ordering::SeqCst) {
            println!("  -> RetryAfter(250ms): notification service unavailable");
            return DeliveryDisposition::RetryAfter(self.retry_delay);
        }
        println!(
            "  -> Acknowledge: notification delivered for {}/{}",
            notification.fleet_id, notification.bicycle_id
        );
        self.successes.fetch_add(1, Ordering::SeqCst);
        DeliveryDisposition::Acknowledge
    }
}

struct Demo {
    connection: NatsConnection,
    messaging: ApplicationMessagingConfig,
    consumer_config: ConsumerConfig<CommandAddress>,
    quarantine_subject: String,
    handler: Arc<NotificationHandler>,
}

impl Demo {
    async fn connect(url: String) -> DemoResult<Self> {
        let application =
            ApplicationName::new(format!("bike-quarantine-{}", Uuid::now_v7().simple()))?;
        let context = application.test_bounded_context("bike-rental")?;
        let messaging = ApplicationMessagingConfig::new_in_scope(&application, TrafficScope::Test)?
            .with_max_bytes(8 * 1024 * 1024)?;
        let consumer_config = ConsumerConfig::new(
            context.consumer_name("notify-rental", 1)?,
            context.durable_name("notify-rental", 1)?,
            context.command_address("notify-rental")?,
            Duration::from_secs(5),
            Duration::from_secs(2),
            1,
            3,
        )?;
        let quarantine_subject =
            format!("{application}.test.quarantine.command.bike-rental.notify-rental");
        let connection = connect(
            &NatsConnectionConfig::new("bike-rental-quarantine-demo", url)
                .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
        )
        .await?;
        Ok(Self {
            connection,
            messaging,
            consumer_config,
            quarantine_subject,
            handler: Arc::new(NotificationHandler::new()?),
        })
    }

    async fn run(&self) -> DemoResult {
        provision_application_messaging(self.connection.jetstream(), &self.messaging).await?;
        let topology = self.messaging.topology();
        provision_durable_consumer(self.connection.jetstream(), topology, &self.consumer_config)
            .await?;
        println!("Command subject: {}", self.consumer_config.address());
        println!("Quarantine subject: {}", self.quarantine_subject);
        println!("Quarantine stream: {}", topology.quarantine_stream());

        let consumer = self
            .connection
            .consumer_factory(topology.clone())
            .create(self.consumer_config.clone())?;
        // Poll the worker alongside the walkthrough so an unexpected worker exit
        // is surfaced immediately and it is stopped before cleanup on every path.
        tokio::select! {
            result = consumer.run(self.handler.clone()) => {
                result?;
                Err(io::Error::other("notification consumer unexpectedly stopped").into())
            }
            result = self.walkthrough() => result,
        }
    }

    async fn walkthrough(&self) -> DemoResult {
        self.retry_and_recover().await?;
        self.invalid_deliveries().await?;
        println!(
            "\nPASS: recovery acknowledged, source queue empty, three quarantine records retained."
        );
        Ok(())
    }

    async fn retry_and_recover(&self) -> DemoResult {
        println!("\n1. Exhaust three deliveries while the notification service is down.");
        let original_id = MessageId::new("notification-outage")?;
        let mut metadata = CallerMetadata::new();
        metadata.insert("x-demo-scenario", "notification-outage")?;
        let source_sequence = self
            .publish(original_id.clone(), PAYLOAD.to_vec(), metadata.clone())
            .await?;
        self.wait_for_settlement(1).await?;
        let record = self
            .inspect_record(
                "maximum delivery attempts exceeded",
                QuarantineFailureKind::DeliveryAttemptsExhausted,
                3,
            )
            .await?;
        ensure(
            record.message_id() == original_id.as_str(),
            "original message identity",
        )?;
        ensure(
            record.source_sequence() == source_sequence,
            "original stream sequence",
        )?;
        ensure(record.metadata() == &metadata, "preserved caller metadata")?;
        ensure(
            BASE64.decode(record.payload_base64())? == PAYLOAD,
            "preserved payload",
        )?;
        ensure(
            self.handler.calls.load(Ordering::SeqCst) == 3,
            "three handler attempts",
        )?;

        println!("\n2. Repair the service and explicitly republish the quarantined payload.");
        self.handler.service_available.store(true, Ordering::SeqCst);
        ensure(
            !record.payload_truncated(),
            "complete payload required for republication",
        )?;
        // A new ID avoids the broker deduplicating the recovery publication.
        // Keep the original record and link the new delivery back to it.
        let mut recovery_metadata = record.metadata().clone();
        recovery_metadata.insert("x-redrive-of", record.message_id())?;
        self.publish(
            MessageId::new("notification-recovery")?,
            BASE64.decode(record.payload_base64())?,
            recovery_metadata,
        )
        .await?;
        self.wait_for_settlement(1).await?;
        ensure(
            self.handler.calls.load(Ordering::SeqCst) == 4,
            "one recovery delivery",
        )?;
        ensure(
            self.handler.successes.load(Ordering::SeqCst) == 1,
            "successful recovery",
        )?;
        println!("  Original quarantine record retained; recovered command acknowledged.");
        Ok(())
    }

    async fn invalid_deliveries(&self) -> DemoResult {
        println!("\n3. Quarantine an invalid application payload immediately.");
        self.publish(
            MessageId::new("invalid-notification")?,
            INVALID_PAYLOAD.to_vec(),
            CallerMetadata::new(),
        )
        .await?;
        self.wait_for_settlement(2).await?;
        let invalid = self
            .inspect_record(INVALID_REASON, QuarantineFailureKind::HandlerFailure, 1)
            .await?;
        ensure(
            invalid.message_id() == "invalid-notification",
            "invalid payload identity",
        )?;
        ensure(
            self.handler.calls.load(Ordering::SeqCst) == 5,
            "immediate quarantine",
        )?;

        println!("\n4. Quarantine a raw message missing the required transport headers.");
        // Bypass NatsPublisher deliberately: no Content-Type or Nats-Msg-Id.
        self.connection
            .jetstream()
            .publish(
                self.consumer_config.address().as_str().to_owned(),
                PAYLOAD.into(),
            )
            .await?
            .await?;
        self.wait_for_settlement(3).await?;
        let raw = self
            .inspect_record(
                "invalid source message",
                QuarantineFailureKind::InvalidSourceMessage,
                1,
            )
            .await?;
        ensure(
            raw.message_id() == "missing-or-invalid",
            "invalid transport identity",
        )?;
        ensure(
            self.handler.calls.load(Ordering::SeqCst) == 5,
            "invalid transport bypasses handler",
        )?;
        Ok(())
    }

    async fn publish(
        &self,
        id: MessageId,
        payload: Vec<u8>,
        metadata: CallerMetadata,
    ) -> DemoResult<u64> {
        let message = OutboundMessage::new(self.consumer_config.address().clone(), id, payload)?
            .with_metadata(metadata);
        let ack = self
            .connection
            .publisher(self.messaging.topology().clone())
            .publish_command_with_ack(message, Duration::from_secs(5))
            .await?;
        ensure(!ack.duplicate(), "fresh publication")?;
        println!(
            "  command PubAck: stream={}, sequence={}",
            ack.stream(),
            ack.sequence()
        );
        Ok(ack.sequence())
    }

    async fn wait_for_settlement(&self, expected_records: u64) -> DemoResult {
        let topology = self.messaging.topology();
        let mut source = self
            .connection
            .jetstream()
            .get_stream(topology.command_stream().as_str())
            .await?;
        let mut quarantine = self
            .connection
            .jetstream()
            .get_stream(topology.quarantine_stream().as_str())
            .await?;
        timeout(WAIT, async {
            loop {
                let records = quarantine.info().await?.state.messages;
                let pending = source.info().await?.state.messages;
                ensure(records <= expected_records, "no extra quarantine records")?;
                if records == expected_records && pending == 0 {
                    println!("  settled: source messages=0, quarantine records={records}");
                    return Ok::<_, Box<dyn Error + Send + Sync>>(());
                }
                sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| io::Error::other("timed out waiting for source ACK/TERM and quarantine"))??;
        Ok(())
    }

    async fn inspect_record(
        &self,
        reason: &str,
        failure_kind: QuarantineFailureKind,
        attempt: u32,
    ) -> DemoResult<QuarantineRecord> {
        let stream = self
            .connection
            .jetstream()
            .get_stream(self.messaging.topology().quarantine_stream().as_str())
            .await?;
        let stored = stream
            .get_last_raw_message_by_subject(&self.quarantine_subject)
            .await?;
        let record: QuarantineRecord = serde_json::from_slice(&stored.payload)?;
        ensure(record.reason() == reason, "quarantine reason")?;
        ensure(
            record.failure_kind() == Some(failure_kind),
            "quarantine failure category",
        )?;
        ensure(record.attempt() == attempt, "quarantine attempt")?;
        ensure(
            record.address() == self.consumer_config.address().as_str(),
            "source address",
        )?;
        ensure(!record.payload_truncated(), "full demo payload retained")?;
        println!(
            "  stored quarantine record:\n{}",
            serde_json::to_string_pretty(&record)?
        );
        println!(
            "  decoded payload: {}",
            String::from_utf8(BASE64.decode(record.payload_base64())?)?
        );
        Ok(record)
    }

    async fn cleanup(&self) -> DemoResult {
        for stream in self.messaging.streams() {
            self.connection
                .delete_stream_if_exists(stream.name().as_str())
                .await?;
        }
        println!("Removed this run's demo streams.");
        Ok(())
    }

    fn print_inspection_commands(&self) {
        println!("\nStreams retained. With the NATS CLI, inspect them using:");
        println!(
            "  nats --server \"$ROSTFREI_NATS_URL\" stream view {}",
            self.messaging.topology().quarantine_stream()
        );
        println!("Remove this run's streams when finished:");
        for stream in self.messaging.streams() {
            println!(
                "  nats --server \"$ROSTFREI_NATS_URL\" stream rm {} --force",
                stream.name()
            );
        }
    }
}

fn ensure(condition: bool, description: &str) -> DemoResult {
    if !condition {
        return Err(io::Error::other(format!("demo check failed: {description}")).into());
    }
    Ok(())
}

async fn run(url: String, keep_streams: bool) -> DemoResult {
    let demo = Demo::connect(url).await?;
    let result = demo.run().await;
    let cleanup = if keep_streams {
        demo.print_inspection_commands();
        Ok(())
    } else {
        demo.cleanup().await
    };
    let drain = demo.connection.drain().await;
    result?;
    cleanup?;
    drain?;
    Ok(())
}

#[tokio::main]
async fn main() -> DemoResult {
    let mut keep_streams = false;
    for argument in env::args().skip(1) {
        match argument.as_str() {
            "--keep-streams" => keep_streams = true,
            "--help" | "-h" => {
                println!("Usage: bike-rental-quarantine-demo [--keep-streams]");
                println!("Uses ROSTFREI_NATS_URL (default nats://127.0.0.1:4222).");
                return Ok(());
            }
            _ => return Err(io::Error::other(format!("unknown argument: {argument}")).into()),
        }
    }
    let url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_owned());
    run(url, keep_streams).await
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn quarantine_walkthrough() -> super::DemoResult {
        let url = rostfrei_testing::integration::nats_url()?;
        super::run(url, false).await
    }
}
