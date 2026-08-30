use std::sync::Arc;

use async_nats::{Client, Message, Subscriber};
use async_trait::async_trait;
use futures_util::StreamExt;
use rostfrei_messaging_core::{ApplicationName, CorrelationId, MessageId};

use crate::{error::NatsError, publish::CORRELATION_ID_HEADER};

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
}

impl NatsCorrelationObserver {
    pub const fn new(client: Client, application: ApplicationName) -> Self {
        Self {
            client,
            application,
        }
    }

    pub async fn subscribe(&self) -> Result<NatsCorrelationSubscription, NatsError> {
        let domain_events = self
            .client
            .subscribe(format!("{}.domain.>", self.application.as_str()))
            .await
            .map_err(|_| NatsError::Consumer)?;
        let integration_events = self
            .client
            .subscribe(format!("{}.integration.>", self.application.as_str()))
            .await
            .map_err(|_| NatsError::Consumer)?;
        self.client.flush().await.map_err(|_| NatsError::Flush)?;
        Ok(NatsCorrelationSubscription {
            domain_events,
            integration_events,
        })
    }

    pub async fn run(&self, handler: Arc<dyn CorrelatedMessageHandler>) -> Result<(), NatsError> {
        self.subscribe().await?.run(handler).await
    }
}

pub struct NatsCorrelationSubscription {
    domain_events: Subscriber,
    integration_events: Subscriber,
}

impl NatsCorrelationSubscription {
    pub async fn run(
        mut self,
        handler: Arc<dyn CorrelatedMessageHandler>,
    ) -> Result<(), NatsError> {
        loop {
            tokio::select! {
                message = self.domain_events.next() => {
                    let Some(message) = message else {
                        return Err(NatsError::Consumer);
                    };
                    if let Some(message) = correlated_message(&message, CorrelatedMessageFamily::DomainEvent) {
                        handler.handle(message).await;
                    }
                }
                message = self.integration_events.next() => {
                    let Some(message) = message else {
                        return Err(NatsError::Consumer);
                    };
                    if let Some(message) = correlated_message(&message, CorrelatedMessageFamily::IntegrationEvent) {
                        handler.handle(message).await;
                    }
                }
            }
        }
    }
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
