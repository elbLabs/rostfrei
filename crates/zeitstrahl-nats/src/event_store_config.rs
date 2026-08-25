use std::fmt::Write as _;
use std::time::Duration;

use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
use sha2::{Digest, Sha256};
use zeitstrahl_core::{EventStoreError, EventStoreErrorKind};

const MAX_STREAM_NAME_LEN: usize = 255;
const MAX_SUBJECT_PREFIX_LEN: usize = 512;
const MAX_SUBJECT_TOKEN_LEN: usize = 256;
const MAX_COMMIT_BYTES: usize = 64 * 1024 * 1024;
const DUPLICATE_WINDOW: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsEventStoreConfig {
    stream_name: String,
    subject_prefix: String,
    max_stream_bytes: i64,
    max_commit_bytes: usize,
    replicas: usize,
    puback_timeout: Duration,
}

impl NatsEventStoreConfig {
    pub fn new(
        stream_name: impl Into<String>,
        subject_prefix: impl Into<String>,
        max_stream_bytes: i64,
        max_commit_bytes: usize,
        replicas: usize,
        puback_timeout: Duration,
    ) -> Result<Self, EventStoreError> {
        let config = Self {
            stream_name: stream_name.into(),
            subject_prefix: subject_prefix.into(),
            max_stream_bytes,
            max_commit_bytes,
            replicas,
            puback_timeout,
        };
        config.validate()?;
        Ok(config)
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

    pub const fn max_commit_bytes(&self) -> usize {
        self.max_commit_bytes
    }

    pub const fn replicas(&self) -> usize {
        self.replicas
    }

    pub const fn puback_timeout(&self) -> Duration {
        self.puback_timeout
    }

    pub fn subject_filter(&self) -> String {
        format!("{}.>", self.subject_prefix)
    }

    pub fn aggregate_subject(&self, aggregate_type: &str, aggregate_id: &str) -> String {
        let digest = digest_parts(&[aggregate_type.as_bytes(), aggregate_id.as_bytes()]);
        format!("{}.aggregate.{digest}", self.subject_prefix)
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
            max_message_size: i32::try_from(self.max_commit_bytes)
                .expect("validated commit size fits in i32"),
            max_consumers: -1,
            no_ack: false,
            duplicate_window: DUPLICATE_WINDOW,
            num_replicas: self.replicas,
            deny_delete: true,
            deny_purge: true,
            allow_rollup: false,
            ..Default::default()
        }
    }

    fn validate(&self) -> Result<(), EventStoreError> {
        if !valid_stream_name(&self.stream_name) {
            return Err(invalid("invalid JetStream stream name"));
        }
        if !valid_subject_prefix(&self.subject_prefix) {
            return Err(invalid("subject prefix must contain literal NATS tokens"));
        }
        if self.max_stream_bytes <= 0 {
            return Err(invalid("maximum stream bytes must be finite and positive"));
        }
        if self.max_commit_bytes == 0 || self.max_commit_bytes > MAX_COMMIT_BYTES {
            return Err(invalid(format!(
                "maximum commit bytes must be between 1 and {MAX_COMMIT_BYTES}"
            )));
        }
        if self.max_stream_bytes
            < i64::try_from(self.max_commit_bytes).expect("maximum commit size fits in i64")
        {
            return Err(invalid(
                "maximum stream bytes must be at least maximum commit bytes",
            ));
        }
        if !(1..=5).contains(&self.replicas) {
            return Err(invalid("replicas must be explicitly set between 1 and 5"));
        }
        if self.puback_timeout.is_zero() {
            return Err(invalid("PubAck timeout must be greater than zero"));
        }
        Ok(())
    }
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

fn valid_subject_prefix(value: &str) -> bool {
    value.len() <= MAX_SUBJECT_PREFIX_LEN
        && !value.is_empty()
        && value.split('.').all(valid_subject_token)
}

fn valid_subject_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SUBJECT_TOKEN_LEN
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn digest_parts(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_stream_config_is_append_only_and_finite() {
        let config = NatsEventStoreConfig::new(
            "EVENT_STORE_TEST",
            "private.event-store-test",
            8 * 1024 * 1024,
            1024 * 1024,
            3,
            Duration::from_secs(2),
        )
        .expect("valid config")
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
        assert_eq!(config.num_replicas, 3);
    }

    #[test]
    fn aggregate_subject_is_deterministic_and_opaque() {
        let config = NatsEventStoreConfig::new(
            "EVENT_STORE_TEST",
            "private.event-store-test",
            8 * 1024 * 1024,
            1024 * 1024,
            1,
            Duration::from_secs(2),
        )
        .expect("valid config");
        let subject = config.aggregate_subject("Account", "account-123");
        assert_eq!(subject, config.aggregate_subject("Account", "account-123"));
        assert!(!subject.contains("Account"));
        assert!(!subject.contains("account-123"));
        assert_ne!(subject, config.aggregate_subject("Account", "account-124"));
    }
}
