//! Disposable stream shared by the runnable example and its real-NATS tests.

use std::{io, time::Duration};

use async_nats::jetstream::{self, consumer, response::Response};
use tokio::time::timeout;

use super::protocol::{PinnedReader, Result, admin_unpin, nanos};

pub const DURABLE: &str = "publication";
pub const GROUP: &str = "domain-events";

pub struct Fixture {
    pub context: jetstream::Context,
    pub stream: String,
    pub subject: String,
    url: String,
}

impl Fixture {
    pub async fn create(url: &str, pin_ttl: Duration, ack_wait: Duration) -> Result<Self> {
        let client = async_nats::connect(url).await?;
        let stream = format!("PINNED_HANDOFF{}", client.new_inbox().replace('.', "_"));
        let subject = format!("{stream}.events");
        let context = jetstream::new(client);
        context
            .create_stream(jetstream::stream::Config {
                name: stream.clone(),
                subjects: vec![subject.clone()],
                storage: jetstream::stream::StorageType::Memory,
                max_bytes: 1024 * 1024,
                ..Default::default()
            })
            .await?;
        let fixture = Self {
            context,
            stream,
            subject,
            url: url.to_owned(),
        };
        // Use the public low-level API because async-nats 0.50 does not model
        // priority_timeout. Context retains the configured JetStream prefix.
        let response: Response<consumer::Info> = fixture
            .context
            .request(
                format!("CONSUMER.DURABLE.CREATE.{}.{DURABLE}", fixture.stream),
                &serde_json::json!({
                    "stream_name": fixture.stream,
                    "config": {
                        "durable_name": DURABLE,
                        "name": DURABLE,
                        "deliver_policy": "all",
                        "ack_policy": "explicit",
                        "ack_wait": nanos(ack_wait)?,
                        "max_deliver": -1,
                        "filter_subject": fixture.subject,
                        "max_ack_pending": 100,
                        "max_batch": 100,
                        "max_waiting": 16,
                        "priority_policy": "pinned_client",
                        "priority_groups": [GROUP],
                        "priority_timeout": nanos(pin_ttl)?,
                    }
                }),
            )
            .await?;
        if let Response::Err { error } = response {
            fixture.cleanup().await?;
            return Err(error.into());
        }
        Ok(fixture)
    }

    pub async fn reader(&self) -> Result<PinnedReader> {
        // Independent connections model two overlapping application instances.
        let context = jetstream::new(async_nats::connect(&self.url).await?);
        Ok(PinnedReader::new(context, &self.stream, DURABLE, GROUP))
    }

    pub async fn publish(&self, values: &[&str]) -> Result<()> {
        for value in values {
            self.context
                .publish(self.subject.clone(), value.to_string().into())
                .await?
                .await?;
        }
        Ok(())
    }

    pub async fn info(&self) -> Result<consumer::Info> {
        let stream = self.context.get_stream(&self.stream).await?;
        let consumer: consumer::PullConsumer = stream.get_consumer(DURABLE).await?;
        Ok(consumer.cached_info().clone())
    }

    pub async fn wait_for_standby(&self) -> Result<()> {
        timeout(Duration::from_secs(2), async {
            loop {
                if self.info().await?.num_waiting > 0 {
                    return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        Ok(())
    }

    pub async fn unpin(&self) -> Result<()> {
        admin_unpin(&self.context, &self.stream, DURABLE, GROUP).await
    }

    pub async fn cleanup(&self) -> Result<()> {
        self.context.delete_stream(&self.stream).await?;
        Ok(())
    }
}

pub fn payloads(messages: &[jetstream::Message]) -> Result<Vec<&str>> {
    messages
        .iter()
        .map(|message| std::str::from_utf8(&message.payload).map_err(Into::into))
        .collect()
}

pub fn require(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(io::Error::other(message.to_owned()).into());
    }
    Ok(())
}
