use std::{sync::Arc, time::Duration};

use async_nats::{
    Client, Message,
    jetstream::{self, consumer, consumer::DeliverPolicy},
};
use async_trait::async_trait;
use futures_util::StreamExt;
use rostfrei_messaging_core::{ApplicationName, CorrelationId, MessageId};
use tokio::time::MissedTickBehavior;

use crate::{error::NatsError, publish::CORRELATION_ID_HEADER};

const STREAM_GENERATION_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    domain_events: String,
    integration_events: String,
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
            domain_events: domain_event_stream.into(),
            integration_events: integration_event_stream.into(),
        });
        self
    }

    pub async fn subscribe(&self) -> Result<NatsCorrelationSubscription, NatsError> {
        let context = jetstream::new(self.client.clone());
        let domain_filter = family_filter(&self.application, CorrelatedMessageFamily::DomainEvent);
        let integration_filter =
            family_filter(&self.application, CorrelatedMessageFamily::IntegrationEvent);
        let stream_names = if let Some(stream_names) = &self.stream_names {
            stream_names.clone()
        } else {
            CorrelationStreamNames {
                domain_events: context
                    .stream_by_subject(domain_filter)
                    .await
                    .map_err(|_| NatsError::StreamNotFound)?,
                integration_events: context
                    .stream_by_subject(integration_filter)
                    .await
                    .map_err(|_| NatsError::StreamNotFound)?,
            }
        };
        let domain_events =
            ObservedStream::subscribe(&context, stream_names.domain_events, DeliverPolicy::New)
                .await?;
        let integration_events = ObservedStream::subscribe(
            &context,
            stream_names.integration_events,
            DeliverPolicy::New,
        )
        .await?;
        Ok(NatsCorrelationSubscription {
            context,
            application: self.application.clone(),
            domain_events,
            integration_events,
        })
    }

    pub async fn run(&self, handler: Arc<dyn CorrelatedMessageHandler>) -> Result<(), NatsError> {
        self.subscribe().await?.run(handler).await
    }
}

pub struct NatsCorrelationSubscription {
    context: jetstream::Context,
    application: ApplicationName,
    domain_events: ObservedStream,
    integration_events: ObservedStream,
}

impl NatsCorrelationSubscription {
    pub async fn run(
        mut self,
        handler: Arc<dyn CorrelatedMessageHandler>,
    ) -> Result<(), NatsError> {
        let mut generation_poll = tokio::time::interval(STREAM_GENERATION_POLL_INTERVAL);
        generation_poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                message = self.domain_events.messages.next() => {
                    let Some(message) = message else {
                        return Err(NatsError::Consumer);
                    };
                    let Ok(message) = message else {
                        continue;
                    };
                    if family_matches(&self.application, message.subject.as_str(), CorrelatedMessageFamily::DomainEvent)
                        && let Some(message) = correlated_message(&message.message, CorrelatedMessageFamily::DomainEvent)
                        && self.domain_events.delivery_is_current(&self.context).await
                    {
                        handler.handle(message).await;
                    }
                }
                message = self.integration_events.messages.next() => {
                    let Some(message) = message else {
                        return Err(NatsError::Consumer);
                    };
                    let Ok(message) = message else {
                        continue;
                    };
                    if family_matches(&self.application, message.subject.as_str(), CorrelatedMessageFamily::IntegrationEvent)
                        && let Some(message) = correlated_message(&message.message, CorrelatedMessageFamily::IntegrationEvent)
                        && self.integration_events.delivery_is_current(&self.context).await
                    {
                        handler.handle(message).await;
                    }
                }
                _ = generation_poll.tick() => {
                    let _ = self.domain_events.refresh_if_recreated(&self.context).await;
                    let _ = self.integration_events.refresh_if_recreated(&self.context).await;
                }
            }
        }
    }
}

struct ObservedStream {
    name: String,
    generation: String,
    messages: consumer::pull::Ordered,
}

impl ObservedStream {
    async fn subscribe(
        context: &jetstream::Context,
        name: String,
        deliver_policy: DeliverPolicy,
    ) -> Result<Self, NatsError> {
        let stream = context
            .get_stream(name.clone())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        let generation = stream.cached_info().created.to_string();
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

    async fn delivery_is_current(&mut self, context: &jetstream::Context) -> bool {
        loop {
            match self.refresh_if_recreated(context).await {
                Ok(recreated) => return !recreated,
                Err(_) => tokio::time::sleep(STREAM_GENERATION_POLL_INTERVAL).await,
            }
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
    let correlation_id = one_header(headers, CORRELATION_ID_HEADER)
        .and_then(|value| CorrelationId::new(value).ok())?;
    let message_id =
        one_header(headers, "Nats-Msg-Id").and_then(|value| MessageId::new(value).ok());
    Some(CorrelatedMessage {
        correlation_id,
        family,
        subject: message.subject.to_string(),
        message_id,
        payload: message.payload.to_vec(),
    })
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
}
