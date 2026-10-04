use std::time::Duration;

use async_nats::jetstream::{self, ErrorCode, consumer::pull, stream::ConsumerErrorKind};
use futures_util::StreamExt;

use super::{EventStoreError, corrupt, unavailable};

const READ_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct HistoryReader {
    messages: Option<pull::Ordered>,
    context: jetstream::Context,
    stream_name: String,
    consumer_name: Option<String>,
}

impl HistoryReader {
    pub(super) async fn new(
        context: &jetstream::Context,
        stream: &jetstream::stream::Stream,
        subject: &str,
    ) -> Result<Self, EventStoreError> {
        let name = context.client().new_inbox().replace('.', "_");
        let mut reader = Self {
            messages: None,
            context: context.clone(),
            stream_name: stream.cached_info().config.name.clone(),
            consumer_name: Some(name.clone()),
        };
        let consumer = stream
            .create_consumer(pull::OrderedConfig {
                name: Some(name),
                filter_subject: subject.to_owned(),
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                replay_policy: jetstream::consumer::ReplayPolicy::Instant,
                ..Default::default()
            })
            .await
            .map_err(|error| unavailable(format!("failed to create history replay: {error}")))?;
        if Some(consumer.cached_info().name.as_str()) != reader.consumer_name.as_deref() {
            return Err(unavailable(
                "history replay has an unexpected consumer name",
            ));
        }
        reader.messages =
            Some(consumer.messages().await.map_err(|error| {
                unavailable(format!("failed to subscribe to history: {error}"))
            })?);
        Ok(reader)
    }

    pub(super) async fn next(&mut self) -> Result<jetstream::Message, EventStoreError> {
        let messages = self
            .messages
            .as_mut()
            .ok_or_else(|| unavailable("history replay subscription is absent"))?;
        let message = tokio::time::timeout(READ_TIMEOUT, messages.next())
            .await
            .map_err(|_| unavailable("history replay timed out"))?
            .ok_or_else(|| unavailable("history replay ended before its cutoff"))?
            .map_err(|error| unavailable(format!("history replay failed: {error}")))?;
        let info = message.info().map_err(|error| {
            corrupt(format!(
                "history replay has invalid delivery metadata: {error}"
            ))
        })?;
        if info.stream != self.stream_name || Some(info.consumer) != self.consumer_name.as_deref() {
            return Err(corrupt(
                "history replay belongs to another stream or consumer",
            ));
        }
        Ok(message)
    }

    pub(super) async fn close(&mut self) -> Result<(), EventStoreError> {
        self.messages.take();
        if let Some(name) = self.consumer_name.as_deref() {
            delete_consumer(&self.context, name, &self.stream_name).await?;
            self.consumer_name = None;
        }
        Ok(())
    }
}

async fn delete_consumer(
    context: &jetstream::Context,
    name: &str,
    stream: &str,
) -> Result<(), EventStoreError> {
    match context.delete_consumer_from_stream(name, stream).await {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(error.kind(), ConsumerErrorKind::JetStream(error)
                if error.error_code() == ErrorCode::CONSUMER_NOT_FOUND) =>
        {
            Ok(())
        }
        Err(error) => Err(unavailable(format!(
            "failed to delete history replay: {error}"
        ))),
    }
}

impl Drop for HistoryReader {
    fn drop(&mut self) {
        self.messages.take();
        if let Some(name) = self.consumer_name.take() {
            let context = self.context.clone();
            let stream = self.stream_name.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    if let Err(error) = delete_consumer(&context, &name, &stream).await {
                        tracing::warn!(%error, "failed to clean up cancelled history replay");
                    }
                });
            }
        }
    }
}
