use std::{collections::HashSet, sync::Arc, time::Duration};

use async_nats::{
    Client, Message,
    jetstream::{self, consumer, consumer::DeliverPolicy},
};
use async_trait::async_trait;
use futures_util::StreamExt;
use rostfrei_messaging_core::{ApplicationName, CorrelationId, MessageId};
use tokio::{task::JoinSet, time::MissedTickBehavior};

use crate::{
    error::NatsError,
    publish::{CONTENT_TYPE_HEADER, CORRELATION_ID_HEADER, JSON_CONTENT_TYPE},
};

const STREAM_GENERATION_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CorrelatedMessageFamily {
    DomainEvent,
    IntegrationEvent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelatedMessage {
    correlation_id: CorrelationId,
    family: CorrelatedMessageFamily,
    subject: String,
    message_id: Option<MessageId>,
    headers: async_nats::HeaderMap,
    payload: Vec<u8>,
}

impl CorrelatedMessage {
    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn family(&self) -> CorrelatedMessageFamily {
        self.family
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub const fn message_id(&self) -> Option<&MessageId> {
        self.message_id.as_ref()
    }

    pub const fn headers(&self) -> &async_nats::HeaderMap {
        &self.headers
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[async_trait]
pub trait CorrelatedMessageHandler: Send + Sync {
    async fn handle(&self, message: CorrelatedMessage);
}

#[derive(Clone)]
pub struct NatsCorrelationObserver {
    client: Client,
    application: ApplicationName,
    stream_names: Option<CorrelationStreamNames>,
}

#[derive(Clone)]
struct CorrelationStreamNames {
    domain_events: Vec<String>,
    integration_events: Vec<String>,
}

impl NatsCorrelationObserver {
    pub const fn new(client: Client, application: ApplicationName) -> Self {
        Self {
            client,
            application,
            stream_names: None,
        }
    }

    #[must_use]
    pub fn with_streams(
        mut self,
        domain_event_stream: impl Into<String>,
        integration_event_stream: impl Into<String>,
    ) -> Self {
        self.stream_names = Some(CorrelationStreamNames {
            domain_events: vec![domain_event_stream.into()],
            integration_events: vec![integration_event_stream.into()],
        });
        self
    }

    pub async fn subscribe(&self) -> Result<NatsCorrelationSubscription, NatsError> {
        let context = jetstream::new(self.client.clone());
        let stream_names = if let Some(stream_names) = &self.stream_names {
            stream_names.clone()
        } else {
            CorrelationStreamNames {
                domain_events: streams_for_family(
                    &context,
                    &self.application,
                    CorrelatedMessageFamily::DomainEvent,
                )
                .await?,
                integration_events: streams_for_family(
                    &context,
                    &self.application,
                    CorrelatedMessageFamily::IntegrationEvent,
                )
                .await?,
            }
        };
        let mut streams = Vec::with_capacity(
            stream_names
                .domain_events
                .len()
                .saturating_add(stream_names.integration_events.len()),
        );
        for name in stream_names.domain_events {
            streams.push((
                ObservedStream::subscribe(&context, name, DeliverPolicy::New).await?,
                CorrelatedMessageFamily::DomainEvent,
            ));
        }
        for name in stream_names.integration_events {
            streams.push((
                ObservedStream::subscribe(&context, name, DeliverPolicy::New).await?,
                CorrelatedMessageFamily::IntegrationEvent,
            ));
        }
        Ok(NatsCorrelationSubscription {
            context,
            application: self.application.clone(),
            streams,
            discover_streams: self.stream_names.is_none(),
        })
    }

    pub async fn run(&self, handler: Arc<dyn CorrelatedMessageHandler>) -> Result<(), NatsError> {
        self.subscribe().await?.run(handler).await
    }
}

pub struct NatsCorrelationSubscription {
    context: jetstream::Context,
    application: ApplicationName,
    streams: Vec<(ObservedStream, CorrelatedMessageFamily)>,
    discover_streams: bool,
}

impl NatsCorrelationSubscription {
    pub async fn run(self, handler: Arc<dyn CorrelatedMessageHandler>) -> Result<(), NatsError> {
        let mut workers = JoinSet::new();
        let mut observed = HashSet::new();
        for (stream, family) in self.streams {
            observed.insert((stream.name.clone(), family));
            workers.spawn(stream.run(
                self.context.clone(),
                self.application.clone(),
                family,
                Arc::clone(&handler),
            ));
        }
        let mut discovery = tokio::time::interval(STREAM_GENERATION_POLL_INTERVAL);
        discovery.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                outcome = workers.join_next(), if !workers.is_empty() => {
                    return match outcome {
                        Some(Ok(result)) => result,
                        Some(Err(_)) | None => Err(NatsError::Consumer),
                    };
                }
                _ = discovery.tick(), if self.discover_streams => {
                    for family in [
                        CorrelatedMessageFamily::DomainEvent,
                        CorrelatedMessageFamily::IntegrationEvent,
                    ] {
                        let Ok(names) = streams_for_family(
                            &self.context,
                            &self.application,
                            family,
                        ).await else {
                            continue;
                        };
                        for name in names {
                            if observed.contains(&(name.clone(), family)) {
                                continue;
                            }
                            let Ok(stream) = ObservedStream::subscribe(
                                &self.context,
                                name.clone(),
                                DeliverPolicy::All,
                            ).await else {
                                continue;
                            };
                            observed.insert((name, family));
                            workers.spawn(stream.run(
                                self.context.clone(),
                                self.application.clone(),
                                family,
                                Arc::clone(&handler),
                            ));
                        }
                    }
                }
            }
        }
    }
}

struct ObservedStream {
    name: String,
    generation: String,
    next_stream_sequence: u64,
    messages: consumer::pull::Ordered,
}

impl ObservedStream {
    async fn run(
        mut self,
        context: jetstream::Context,
        application: ApplicationName,
        family: CorrelatedMessageFamily,
        handler: Arc<dyn CorrelatedMessageHandler>,
    ) -> Result<(), NatsError> {
        let mut generation_poll = tokio::time::interval(STREAM_GENERATION_POLL_INTERVAL);
        generation_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                message = self.messages.next() => {
                    let Some(message) = message else {
                        return Err(NatsError::Consumer);
                    };
                    let Ok(message) = message else {
                        continue;
                    };
                    if self.refresh_if_recreated(&context).await.unwrap_or(false) {
                        continue;
                    }
                    let Ok(info) = message.info() else {
                        continue;
                    };
                    if info.stream != self.name || info.stream_sequence < self.next_stream_sequence {
                        continue;
                    }
                    self.next_stream_sequence = info.stream_sequence.saturating_add(1);
                    if family_matches(&application, message.subject.as_str(), family)
                        && let Some(message) = correlated_message(&message.message, family)
                    {
                        handler.handle(message).await;
                    }
                }
                _ = generation_poll.tick() => {
                    let _ = self.refresh_if_recreated(&context).await;
                }
            }
        }
    }

    async fn subscribe(
        context: &jetstream::Context,
        name: String,
        deliver_policy: DeliverPolicy,
    ) -> Result<Self, NatsError> {
        let stream = context
            .get_stream(name.clone())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        let info = stream.cached_info();
        let generation = info.created.to_string();
        let next_stream_sequence = if deliver_policy == DeliverPolicy::New {
            info.state.last_sequence.saturating_add(1)
        } else {
            info.state.first_sequence.max(1)
        };
        let consumer: consumer::OrderedPullConsumer = stream
            .create_consumer(consumer::pull::OrderedConfig {
                description: Some("rostfrei correlation observer".to_owned()),
                deliver_policy,
                ..Default::default()
            })
            .await
            .map_err(|_| NatsError::Consumer)?;
        let messages = consumer.messages().await.map_err(|_| NatsError::Consumer)?;
        Ok(Self {
            name,
            generation,
            next_stream_sequence,
            messages,
        })
    }

    async fn refresh_if_recreated(
        &mut self,
        context: &jetstream::Context,
    ) -> Result<bool, NatsError> {
        let stream = context
            .get_stream(self.name.clone())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        if stream.cached_info().created.to_string() != self.generation {
            // Every message in a replacement stream is newer than the original subscription.
            *self = Self::subscribe(context, self.name.clone(), DeliverPolicy::All).await?;
            return Ok(true);
        }
        Ok(false)
    }
}

async fn streams_for_family(
    context: &jetstream::Context,
    application: &ApplicationName,
    family: CorrelatedMessageFamily,
) -> Result<Vec<String>, NatsError> {
    let filter = family_filter(application, family);
    let mut streams = context.streams();
    let mut names = Vec::new();
    while let Some(stream) = streams.next().await {
        let stream = stream.map_err(|_| NatsError::Consumer)?;
        if stream
            .config
            .subjects
            .iter()
            .any(|subject| subject_patterns_intersect(subject, &filter))
        {
            names.push(stream.config.name);
        }
    }
    Ok(names)
}

fn subject_patterns_intersect(left: &str, right: &str) -> bool {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        match (left.next(), right.next()) {
            (Some(">"), Some(_)) | (Some(_), Some(">")) | (None, None) => return true,
            (Some(left), Some(right)) if left == "*" || right == "*" || left == right => {}
            _ => return false,
        }
    }
}

fn family_filter(application: &ApplicationName, family: CorrelatedMessageFamily) -> String {
    let token = match family {
        CorrelatedMessageFamily::DomainEvent => "domain",
        CorrelatedMessageFamily::IntegrationEvent => "integration",
    };
    format!("{}.{token}.>", application.as_str())
}

fn family_matches(
    application: &ApplicationName,
    subject: &str,
    family: CorrelatedMessageFamily,
) -> bool {
    let filter = family_filter(application, family);
    let Some(prefix) = filter.strip_suffix('>') else {
        return false;
    };
    subject
        .strip_prefix(prefix)
        .is_some_and(|remainder| !remainder.is_empty())
}

fn correlated_message(
    message: &Message,
    family: CorrelatedMessageFamily,
) -> Option<CorrelatedMessage> {
    let headers = message.headers.as_ref()?;
    if one_header(headers, CONTENT_TYPE_HEADER) != Some(JSON_CONTENT_TYPE) {
        return None;
    }
    let correlation_id = one_header(headers, CORRELATION_ID_HEADER)
        .and_then(|value| CorrelationId::new(value).ok())?;
    let message_id =
        one_header(headers, "Nats-Msg-Id").and_then(|value| MessageId::new(value).ok());
    let (payload_message_id, payload_correlation_id) =
        payload_identities(&message.payload, family)?;
    if message_id
        .as_ref()
        .is_some_and(|message_id| payload_message_id != message_id.as_str())
        || (family == CorrelatedMessageFamily::IntegrationEvent && message_id.is_none())
        || payload_correlation_id != correlation_id.as_str()
    {
        return None;
    }
    let message_id = message_id.or_else(|| MessageId::new(payload_message_id).ok())?;
    Some(CorrelatedMessage {
        correlation_id,
        family,
        subject: message.subject.to_string(),
        message_id: Some(message_id),
        headers: headers.clone(),
        payload: message.payload.to_vec(),
    })
}

fn payload_identities(payload: &[u8], family: CorrelatedMessageFamily) -> Option<(String, String)> {
    let payload: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let (message_id, correlation_id) = match family {
        CorrelatedMessageFamily::DomainEvent => (
            payload.pointer("/event/eventId")?.as_str()?,
            payload.pointer("/event/correlationId")?.as_str()?,
        ),
        CorrelatedMessageFamily::IntegrationEvent => (
            payload.get("message_id")?.as_str()?,
            payload.get("correlation_id")?.as_str()?,
        ),
    };
    Some((message_id.to_owned(), correlation_id.to_owned()))
}

fn one_header<'a>(headers: &'a async_nats::HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name.to_owned());
    let value = values.next()?.as_str();
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_filtering_remains_application_scoped() {
        let application = ApplicationName::new("bike-rental-test").expect("application name");

        assert!(family_matches(
            &application,
            "bike-rental-test.domain.bike-rental.aggregate.123",
            CorrelatedMessageFamily::DomainEvent,
        ));
        assert!(family_matches(
            &application,
            "bike-rental-test.integration.bicycle-rental-started",
            CorrelatedMessageFamily::IntegrationEvent,
        ));
        assert!(!family_matches(
            &application,
            "bike-rental-prod.domain.bike-rental.aggregate.123",
            CorrelatedMessageFamily::DomainEvent,
        ));
        assert!(!family_matches(
            &application,
            "bike-rental-test.command.rent-bicycle",
            CorrelatedMessageFamily::DomainEvent,
        ));
    }

    #[test]
    fn stream_subject_intersection_covers_scoped_and_wildcard_streams() {
        assert!(subject_patterns_intersect(
            "bike.domain.rental.>",
            "bike.domain.>"
        ));
        assert!(subject_patterns_intersect("bike.>", "bike.domain.>"));
        assert!(subject_patterns_intersect("*.domain.>", "bike.domain.>"));
        assert!(!subject_patterns_intersect(
            "bike.integration.>",
            "bike.domain.>"
        ));
        assert!(!subject_patterns_intersect("bike.domain", "bike.domain.>"));
    }

    #[test]
    fn correlation_payload_identities_are_family_specific() {
        assert_eq!(
            payload_identities(
                br#"{"event":{"eventId":"domain-1","correlationId":"correlation-1"}}"#,
                CorrelatedMessageFamily::DomainEvent,
            ),
            Some(("domain-1".to_owned(), "correlation-1".to_owned()))
        );
        assert_eq!(
            payload_identities(
                br#"{"message_id":"integration-1","correlation_id":"correlation-1"}"#,
                CorrelatedMessageFamily::IntegrationEvent,
            ),
            Some(("integration-1".to_owned(), "correlation-1".to_owned()))
        );
        assert_eq!(
            payload_identities(
                br#"{"message_id":"integration-1"}"#,
                CorrelatedMessageFamily::IntegrationEvent,
            ),
            None
        );
    }
}
