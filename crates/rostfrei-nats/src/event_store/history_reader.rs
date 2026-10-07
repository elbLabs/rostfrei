use std::time::Duration;

use async_nats::{StatusCode, Subject, Subscriber, jetstream};
use futures_util::StreamExt;

use super::{EventStoreError, corrupt, unavailable};

const BATCH_MESSAGES: usize = 256;
const BATCH_BYTES: usize = 8 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const INACTIVE_THRESHOLD: Duration = Duration::from_secs(30);

/// A private replay cursor, never a durable application consumer. No delivery ACKs are sent.
pub(super) struct HistoryReader {
    consumer: jetstream::consumer::PullConsumer,
    context: jetstream::Context,
    stream_name: String,
    consumer_name: Option<String>,
    subscription: Option<Subscriber>,
    inbox: Subject,
    remaining: usize,
    delivered_in_batch: bool,
    batch_bytes: usize,
    next_delivery_sequence: u64,
}

impl HistoryReader {
    pub(super) async fn new(
        context: &jetstream::Context,
        stream: &jetstream::stream::Stream,
        subject: &str,
    ) -> Result<Self, EventStoreError> {
        // Include delivery/header overhead and preserve readability of older, larger events.
        let maximum_message_bytes = usize::try_from(stream.cached_info().config.max_message_size)
            .unwrap_or(super::MAX_SUPPORTED_EVENT_BYTES);
        let batch_bytes = maximum_message_bytes
            .checked_add(64 * 1024)
            .ok_or_else(|| unavailable("history replay byte limit overflowed"))?
            .max(BATCH_BYTES);
        let consumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                filter_subject: subject.to_owned(),
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                ack_policy: jetstream::consumer::AckPolicy::None,
                replay_policy: jetstream::consumer::ReplayPolicy::Instant,
                memory_storage: true,
                num_replicas: 1,
                inactive_threshold: INACTIVE_THRESHOLD,
                ..Default::default()
            })
            .await
            .map_err(|error| unavailable(format!("failed to create history replay: {error}")))?;
        let inbox = Subject::from(context.client().new_inbox());
        let mut reader = Self {
            consumer_name: Some(consumer.cached_info().name.clone()),
            stream_name: stream.cached_info().config.name.clone(),
            consumer,
            context: context.clone(),
            subscription: None,
            inbox,
            remaining: 0,
            delivered_in_batch: false,
            batch_bytes,
            next_delivery_sequence: 1,
        };
        // Install the cleanup guard before another cancellation/failure point.
        reader.subscription = Some(
            context
                .client()
                .subscribe(reader.inbox.clone())
                .await
                .map_err(|error| unavailable(format!("failed to subscribe to history: {error}")))?,
        );
        Ok(reader)
    }

    pub(super) async fn next(&mut self) -> Result<Option<jetstream::Message>, EventStoreError> {
        let subscription = self
            .subscription
            .as_mut()
            .ok_or_else(|| unavailable("history replay subscription is absent"))?;
        loop {
            if self.remaining == 0 && !self.delivered_in_batch {
                self.consumer
                    .request_batch(
                        jetstream::consumer::pull::BatchConfig {
                            batch: BATCH_MESSAGES,
                            max_bytes: self.batch_bytes,
                            no_wait: true,
                            expires: Some(READ_TIMEOUT),
                            ..Default::default()
                        },
                        self.inbox.clone(),
                    )
                    .await
                    .map_err(|error| unavailable(format!("failed to request history: {error}")))?;
                self.remaining = BATCH_MESSAGES;
                self.delivered_in_batch = false;
            }
            let message = tokio::time::timeout(READ_TIMEOUT, subscription.next())
                .await
                .map_err(|_| unavailable("history replay timed out"))?
                .ok_or_else(|| unavailable("history replay subscription closed"))?;
            match message.status.unwrap_or(StatusCode::OK) {
                StatusCode::OK => {
                    if self.remaining == 0 {
                        return Err(corrupt("history replay exceeded its message-count budget"));
                    }
                    self.remaining = self.remaining.saturating_sub(1);
                    self.delivered_in_batch = true;
                    let message = jetstream::Message {
                        message,
                        context: self.context.clone(),
                    };
                    let info = message.info().map_err(|error| {
                        corrupt(format!(
                            "history replay has invalid delivery metadata: {error}"
                        ))
                    })?;
                    if info.stream != self.stream_name
                        || Some(info.consumer) != self.consumer_name.as_deref()
                    {
                        return Err(corrupt(
                            "history replay belongs to another stream or consumer",
                        ));
                    }
                    advance_delivery_sequence(
                        &mut self.next_delivery_sequence,
                        info.consumer_sequence,
                    )?;
                    return Ok(Some(message));
                }
                StatusCode::NOT_FOUND => return Ok(None),
                StatusCode::REQUEST_TERMINATED
                    if matches!(
                        message.description.as_deref(),
                        Some("Batch Completed" | "Message Size Exceeds MaxBytes")
                    ) && self.delivered_in_batch =>
                {
                    // The byte budget can finish a page before its message-count budget.
                    self.remaining = 0;
                    self.delivered_in_batch = false;
                }
                status => {
                    return Err(unavailable(format!(
                        "history replay failed: {status} {:?}",
                        message.description
                    )));
                }
            }
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), EventStoreError> {
        if let Some(name) = self.consumer_name.as_deref() {
            self.context
                .delete_consumer_from_stream(name, &self.stream_name)
                .await
                .map_err(|error| {
                    unavailable(format!("failed to delete history replay: {error}"))
                })?;
            self.consumer_name = None;
        }
        Ok(())
    }
}

fn advance_delivery_sequence(expected: &mut u64, received: u64) -> Result<(), EventStoreError> {
    if received != *expected {
        // ACK-free replay can lose an in-flight message across transport failure. Retry with a
        // fresh cursor rather than misclassifying a delivery gap as corrupt authoritative data.
        return Err(unavailable(
            "history replay delivery sequence is discontinuous",
        ));
    }
    *expected = expected
        .checked_add(1)
        .ok_or_else(|| unavailable("history replay delivery sequence overflowed"))?;
    Ok(())
}

impl Drop for HistoryReader {
    fn drop(&mut self) {
        if let Some(name) = self.consumer_name.take() {
            let context = self.context.clone();
            let stream = self.stream_name.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    if let Err(error) = context.delete_consumer_from_stream(&name, &stream).await {
                        tracing::warn!(%error, "failed to clean up cancelled history replay");
                    }
                });
            }
            // Server-side inactivity cleanup also covers process/runtime loss and lost replies.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rostfrei_core::EventStoreErrorKind;

    #[test]
    fn replay_delivery_gaps_are_retryable_not_corrupt_history() {
        let mut expected = 1;
        assert!(advance_delivery_sequence(&mut expected, 1).is_ok());
        assert_eq!(expected, 2);
        for received in [1, 3] {
            let outcome = advance_delivery_sequence(&mut expected, received);
            assert!(
                matches!(outcome, Err(error) if error.kind() == EventStoreErrorKind::Unavailable)
            );
            assert_eq!(expected, 2);
        }
        assert!(advance_delivery_sequence(&mut expected, 2).is_ok());
        assert_eq!(expected, 3);
    }
}
