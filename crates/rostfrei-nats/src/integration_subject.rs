use async_nats::HeaderMap;
use rostfrei_messaging_core::{
    CausationId, CorrelationId, IntegrationEventAddress, IntegrationEventEnvelope, OutboundMessage,
    TrafficScope,
};
use serde_json::Value;

use crate::{
    error::NatsError,
    messaging_config::SubjectFilter,
    publish::{CONTENT_TYPE_HEADER, CORRELATION_ID_HEADER, JSON_CONTENT_TYPE},
    subject_tokens::{decode, encode, optional},
};

/// Physical integration transport address. Business routes and durable identities
/// continue to use [`IntegrationEventAddress`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationEventSubject {
    subject: String,
    address: IntegrationEventAddress,
    correlation_id: CorrelationId,
    causation_id: Option<CausationId>,
}

impl IntegrationEventSubject {
    pub fn new(
        address: IntegrationEventAddress,
        correlation_id: CorrelationId,
        causation_id: Option<CausationId>,
    ) -> Result<Self, NatsError> {
        let subject = format!(
            "{}.{}.{}",
            address.as_str(),
            encode(correlation_id.as_str()),
            optional(causation_id.as_ref().map(CausationId::as_str)),
        );
        SubjectFilter::new(subject.clone()).map_err(|_| NatsError::InvalidMessage)?;
        Ok(Self {
            subject,
            address,
            correlation_id,
            causation_id,
        })
    }

    pub fn parse(subject: &str) -> Result<Self, NatsError> {
        let (prefix, causation) = subject.rsplit_once('.').ok_or(NatsError::InvalidMessage)?;
        let (address, correlation) = prefix.rsplit_once('.').ok_or(NatsError::InvalidMessage)?;
        let address =
            IntegrationEventAddress::parse(address).map_err(|_| NatsError::InvalidMessage)?;
        let correlation = CorrelationId::new(decode(correlation).ok_or(NatsError::InvalidMessage)?)
            .map_err(|_| NatsError::InvalidMessage)?;
        let causation = if causation == "~" {
            None
        } else {
            Some(
                CausationId::new(decode(causation).ok_or(NatsError::InvalidMessage)?)
                    .map_err(|_| NatsError::InvalidMessage)?,
            )
        };
        Self::new(address, correlation, causation)
    }

    pub fn as_str(&self) -> &str {
        &self.subject
    }

    pub const fn address(&self) -> &IntegrationEventAddress {
        &self.address
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    /// Includes malformed suffixes so workers can quarantine them rather than
    /// leaving undeliverable messages in the stream. Decode before application delivery.
    pub fn filter(address: &IntegrationEventAddress) -> String {
        format!("{}.>", address.as_str())
    }
}

/// Extract only the logical route, including from malformed physical suffixes.
/// This is for selecting a worker or quarantine destination, never validation.
pub fn logical_address(subject: &str) -> Result<IntegrationEventAddress, NatsError> {
    let mut tokens = subject.split('.');
    let application = tokens.next().ok_or(NatsError::InvalidMessage)?;
    let second = tokens.next().ok_or(NatsError::InvalidMessage)?;
    let (scope, kind) = if second == "test" {
        (
            TrafficScope::Test,
            tokens.next().ok_or(NatsError::InvalidMessage)?,
        )
    } else {
        (TrafficScope::Normal, second)
    };
    if kind != "integration" {
        return Err(NatsError::InvalidMessage);
    }
    let context = tokens.next().ok_or(NatsError::InvalidMessage)?;
    let name = tokens.next().ok_or(NatsError::InvalidMessage)?;
    IntegrationEventAddress::new_in_scope(application, scope, context, name)
        .map_err(|_| NatsError::InvalidMessage)
}

pub fn outbound_envelope(
    message: &OutboundMessage<IntegrationEventAddress>,
) -> Result<IntegrationEventEnvelope<Value>, NatsError> {
    let envelope: IntegrationEventEnvelope<Value> =
        serde_json::from_slice(message.payload()).map_err(|_| NatsError::InvalidMessage)?;
    if envelope.message_id() != message.message_id()
        || message
            .correlation_id()
            .is_some_and(|id| id != envelope.correlation_id())
    {
        return Err(NatsError::InvalidMessage);
    }
    Ok(envelope)
}

pub fn decode_message(
    subject: &str,
    headers: Option<&HeaderMap>,
    payload: &[u8],
) -> Result<(IntegrationEventSubject, IntegrationEventEnvelope<Value>), NatsError> {
    let subject = IntegrationEventSubject::parse(subject)?;
    let envelope: IntegrationEventEnvelope<Value> =
        serde_json::from_slice(payload).map_err(|_| NatsError::InvalidMessage)?;
    let headers = headers.ok_or(NatsError::InvalidMessage)?;
    if required_header(headers, CONTENT_TYPE_HEADER)? != JSON_CONTENT_TYPE
        || required_header(headers, "Nats-Msg-Id")? != envelope.message_id().as_str()
        || required_header(headers, CORRELATION_ID_HEADER)? != envelope.correlation_id().as_str()
        || subject.correlation_id() != envelope.correlation_id()
        || subject.causation_id() != envelope.causation_id()
    {
        return Err(NatsError::InvalidMessage);
    }
    Ok((subject, envelope))
}

fn required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, NatsError> {
    let mut values = headers.get_all(name.to_owned());
    let first = values.next().ok_or(NatsError::InvalidMessage)?;
    if values.next().is_some() {
        return Err(NatsError::InvalidMessage);
    }
    Ok(first.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_subjects_preserve_scopes_and_distinguish_absence_from_literal_ids() {
        for (subject, correlation, causation) in [
            ("app.integration.orders.placed.Flow_1.~", "Flow_1", None),
            ("app.test.integration.orders.placed.%7E.%7E", "~", Some("~")),
            (
                "app.integration.orders.placed.%257E.cause%2E1",
                "%7E",
                Some("cause.1"),
            ),
        ] {
            let parsed = IntegrationEventSubject::parse(subject).unwrap();
            assert_eq!(parsed.as_str(), subject);
            assert_eq!(parsed.correlation_id().as_str(), correlation);
            assert_eq!(parsed.causation_id().map(CausationId::as_str), causation);
            assert_eq!(logical_address(subject).unwrap(), *parsed.address());
        }
    }

    #[test]
    fn physical_subjects_require_exactly_two_canonical_control_tokens() {
        for suffix in [
            "",
            "flow",
            "flow.cause.extra",
            "flow..cause",
            "flow.~.extra",
            "~.cause",
            "flow.%2f",
            "flow.%41",
            "flow.%FF",
            "flow.%",
            "flow.*",
            "flow.>",
        ] {
            let subject = format!("app.integration.orders.placed.{suffix}");
            assert!(
                IntegrationEventSubject::parse(&subject).is_err(),
                "{subject}"
            );
            assert_eq!(
                logical_address(&subject).unwrap().as_str(),
                "app.integration.orders.placed",
            );
        }
        assert!(IntegrationEventSubject::parse("app.integration.orders.placed").is_err());
        assert!(IntegrationEventSubject::parse("app.command.orders.place.flow.cause").is_err());
    }
}
