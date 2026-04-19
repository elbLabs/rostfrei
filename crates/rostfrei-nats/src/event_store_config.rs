use std::{mem::size_of, time::Duration};

use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
use rostfrei_core::{EventStoreError, EventStoreErrorKind};
use rostfrei_messaging_core::{ApplicationName, BoundedContext, BoundedContextName};
use sha2::{Digest, Sha256};

use crate::hex::encode_lower_hex;

const MAX_STREAM_NAME_LEN: usize = 255;
const MAX_EVENT_BYTES: usize = 64 * 1024 * 1024;
const DUPLICATE_WINDOW: Duration = Duration::from_mins(2);
pub const DEFAULT_EVENT_STORE_MAX_STREAM_BYTES: i64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_EVENT_STORE_MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_EVENT_STORE_REPLICAS: usize = 1;
pub const DEFAULT_EVENT_STORE_PUBACK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EventByteLimit {
    value: usize,
    nats_value: i32,
    comparison_value: i64,
}

impl EventByteLimit {
    const DEFAULT: Self = Self {
        value: DEFAULT_EVENT_STORE_MAX_EVENT_BYTES,
        nats_value: 2 * 1024 * 1024,
        comparison_value: 2 * 1024 * 1024,
    };

    fn new(value: usize) -> Result<Self, EventStoreError> {
        if value == 0 || value > MAX_EVENT_BYTES {
            return Err(invalid(format!(
                "maximum event bytes must be between 1 and {MAX_EVENT_BYTES}"
            )));
        }
        let nats_value = i32::try_from(value)
            .map_err(|_| invalid("maximum event bytes cannot be represented by JetStream"))?;
        let comparison_value = i64::from(nats_value);
        Ok(Self {
            value,
            nats_value,
            comparison_value,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsEventStoreConfig {
    application: ApplicationName,
    bounded_context: BoundedContextName,
    stream_name: String,
    subject_prefix: String,
    max_stream_bytes: i64,
    max_event_bytes: EventByteLimit,
    replicas: usize,
    puback_timeout: Duration,
}

impl NatsEventStoreConfig {
    pub fn for_bounded_context(context: &BoundedContext) -> Result<Self, EventStoreError> {
        let stream_name = domain_event_stream_name(context);
        Self::new(context, stream_name)
    }

    pub fn new(
        context: &BoundedContext,
        stream_name: impl Into<String>,
    ) -> Result<Self, EventStoreError> {
        let subject_prefix = format!(
            "{}.domain.{}",
            context.application().as_str(),
            context.name().as_str()
        );
        let config = Self {
            application: context.application().clone(),
            bounded_context: context.name().clone(),
            stream_name: stream_name.into(),
            subject_prefix,
            max_stream_bytes: DEFAULT_EVENT_STORE_MAX_STREAM_BYTES,
            max_event_bytes: EventByteLimit::DEFAULT,
            replicas: DEFAULT_EVENT_STORE_REPLICAS,
            puback_timeout: DEFAULT_EVENT_STORE_PUBACK_TIMEOUT,
        };
        config.validate()?;
        Ok(config)
    }

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub const fn bounded_context(&self) -> &BoundedContextName {
        &self.bounded_context
    }

    pub fn with_storage_limits(
        mut self,
        max_stream_bytes: i64,
        max_event_bytes: usize,
    ) -> Result<Self, EventStoreError> {
        self.max_stream_bytes = max_stream_bytes;
        self.max_event_bytes = EventByteLimit::new(max_event_bytes)?;
        self.validate()?;
        Ok(self)
    }

    pub fn with_replicas(mut self, replicas: usize) -> Result<Self, EventStoreError> {
        self.replicas = replicas;
        self.validate()?;
        Ok(self)
    }

    pub fn with_puback_timeout(
        mut self,
        puback_timeout: Duration,
    ) -> Result<Self, EventStoreError> {
        self.puback_timeout = puback_timeout;
        self.validate()?;
        Ok(self)
    }

    pub fn stream_name(&self) -> &str {
        &self.stream_name
    }

    pub fn subject_prefix(&self) -> &str {
        &self.subject_prefix
    }

    pub const fn max_stream_bytes(&self) -> i64 {
        self.max_stream_bytes
    }

    pub const fn max_event_bytes(&self) -> usize {
        self.max_event_bytes.value
    }

    pub const fn replicas(&self) -> usize {
        self.replicas
    }

    pub const fn puback_timeout(&self) -> Duration {
        self.puback_timeout
    }

    pub fn subject_filter(&self) -> String {
        self.aggregate_subject_filter()
    }

    pub fn aggregate_subject(&self, aggregate_type: &str, aggregate_id: &str) -> String {
        let digest = digest_parts(&[aggregate_type.as_bytes(), aggregate_id.as_bytes()]);
        format!("{}.aggregate.{digest}", self.subject_prefix)
    }

    pub fn aggregate_subject_filter(&self) -> String {
        format!("{}.aggregate.*", self.subject_prefix)
    }

    pub fn stream_config(&self) -> Config {
        Config {
            name: self.stream_name.clone(),
            subjects: vec![self.subject_filter()],
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            discard: DiscardPolicy::New,
            max_age: Duration::ZERO,
            max_messages: -1,
            max_messages_per_subject: -1,
            max_bytes: self.max_stream_bytes,
            max_message_size: self.max_event_bytes.nats_value,
            max_consumers: -1,
            no_ack: false,
            duplicate_window: DUPLICATE_WINDOW,
            num_replicas: self.replicas,
            deny_delete: true,
            deny_purge: true,
            allow_rollup: false,
            allow_atomic_publish: true,
            ..Default::default()
        }
    }

    fn validate(&self) -> Result<(), EventStoreError> {
        if !valid_stream_name(&self.stream_name) {
            return Err(invalid("invalid JetStream stream name"));
        }
        if self.max_stream_bytes <= 0 {
            return Err(invalid("maximum stream bytes must be finite and positive"));
        }
        if self.max_stream_bytes < self.max_event_bytes.comparison_value {
            return Err(invalid(
                "maximum stream bytes must be at least maximum event bytes",
            ));
        }
        if !(1..=5).contains(&self.replicas) {
            return Err(invalid("replicas must be between 1 and 5"));
        }
        if self.puback_timeout.is_zero() {
            return Err(invalid("PubAck timeout must be greater than zero"));
        }
        Ok(())
    }
}

fn domain_event_stream_name(context: &BoundedContext) -> String {
    format!(
        "{}__{}_DOMAIN_EVENTS",
        stream_token(context.application().as_str()),
        stream_token(context.name().as_str())
    )
}

fn stream_token(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'-' => b'_',
            _ => byte.to_ascii_uppercase(),
        })
        .map(char::from)
        .collect()
}

fn valid_stream_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_STREAM_NAME_LEN
        && value.bytes().all(|byte| {
            !byte.is_ascii_whitespace()
                && !byte.is_ascii_control()
                && !matches!(byte, b'.' | b'*' | b'>')
        })
}

fn digest_parts(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update([0_u8; 8 - size_of::<usize>()]);
        digest.update(part.len().to_be_bytes());
        digest.update(part);
    }
    encode_lower_hex(digest.finalize())
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> BoundedContext {
        ApplicationName::new("fast-inbox")
            .unwrap()
            .bounded_context("commercial-access")
            .unwrap()
    }

    #[test]
    fn bounded_context_derives_event_store_identity_and_policy() {
        let config = NatsEventStoreConfig::for_bounded_context(&context()).unwrap();

        assert_eq!(config.application().as_str(), "fast-inbox");
        assert_eq!(config.bounded_context().as_str(), "commercial-access");
        assert_eq!(
            config.stream_name(),
            "FAST_INBOX__COMMERCIAL_ACCESS_DOMAIN_EVENTS"
        );
        assert_eq!(
            config.subject_prefix(),
            "fast-inbox.domain.commercial-access"
        );
        assert_eq!(
            config.subject_filter(),
            "fast-inbox.domain.commercial-access.aggregate.*"
        );
        assert_eq!(config.subject_filter(), config.aggregate_subject_filter());
    }

    #[test]
    fn domain_event_stream_names_have_an_unambiguous_scope_boundary() {
        let first = ApplicationName::new("foo-bar")
            .unwrap()
            .bounded_context("baz")
            .unwrap();
        let second = ApplicationName::new("foo")
            .unwrap()
            .bounded_context("bar-baz")
            .unwrap();

        assert_ne!(
            domain_event_stream_name(&first),
            domain_event_stream_name(&second)
        );
    }

    #[test]
    fn authoritative_stream_config_is_append_only_and_finite() {
        let config = NatsEventStoreConfig::new(&context(), "EVENT_STORE_TEST")
            .expect("valid config")
            .with_storage_limits(8 * 1024 * 1024, 1024 * 1024)
            .expect("valid storage limits")
            .with_replicas(3)
            .expect("valid replicas")
            .with_puback_timeout(Duration::from_secs(2))
            .expect("valid PubAck timeout")
            .stream_config();

        assert_eq!(config.retention, RetentionPolicy::Limits);
        assert_eq!(config.storage, StorageType::File);
        assert_eq!(config.discard, DiscardPolicy::New);
        assert_eq!(config.max_messages, -1);
        assert_eq!(config.max_messages_per_subject, -1);
        assert_eq!(config.max_bytes, 8 * 1024 * 1024);
        assert_eq!(config.max_message_size, 1024 * 1024);
        assert!(config.max_age.is_zero());
        assert!(!config.no_ack);
        assert!(config.deny_delete);
        assert!(config.deny_purge);
        assert!(!config.allow_rollup);
        assert!(config.allow_atomic_publish);
        assert_eq!(config.num_replicas, 3);
    }

    #[test]
    fn event_store_defaults_are_bounded_and_single_node_compatible() {
        let config = NatsEventStoreConfig::for_bounded_context(&context()).expect("valid config");

        assert_eq!(
            config.max_stream_bytes(),
            DEFAULT_EVENT_STORE_MAX_STREAM_BYTES
        );
        assert_eq!(
            config.max_event_bytes(),
            DEFAULT_EVENT_STORE_MAX_EVENT_BYTES
        );
        assert_eq!(config.replicas(), DEFAULT_EVENT_STORE_REPLICAS);
        assert_eq!(config.puback_timeout(), DEFAULT_EVENT_STORE_PUBACK_TIMEOUT);
    }

    #[test]
    fn aggregate_subject_is_deterministic_and_opaque() {
        let config =
            NatsEventStoreConfig::new(&context(), "EVENT_STORE_TEST").expect("valid config");
        let subject = config.aggregate_subject("Account", "account-123");
        assert_eq!(subject, config.aggregate_subject("Account", "account-123"));
        assert_eq!(
            subject,
            "fast-inbox.domain.commercial-access.aggregate.c747047da35490fdcc850f47802f98b07d5095ea8296688da589d8bf883b4246"
        );
        assert!(!subject.contains("Account"));
        assert!(!subject.contains("account-123"));
        assert_ne!(subject, config.aggregate_subject("Account", "account-124"));
    }

    #[test]
    fn applications_with_the_same_context_have_disjoint_event_stores() {
        let first = NatsEventStoreConfig::for_bounded_context(&context()).unwrap();
        let second_context = ApplicationName::new("other-inbox")
            .unwrap()
            .bounded_context("commercial-access")
            .unwrap();
        let second = NatsEventStoreConfig::for_bounded_context(&second_context).unwrap();

        assert_ne!(first.stream_name(), second.stream_name());
        assert_ne!(first.subject_filter(), second.subject_filter());
    }
}
