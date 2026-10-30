use std::{mem::size_of, time::Duration};

use async_nats::jetstream::stream::{Config, DiscardPolicy, RetentionPolicy, StorageType};
use rostfrei_core::{EventStoreError, EventStoreErrorKind};
use rostfrei_messaging_core::{ApplicationName, BoundedContext, BoundedContextName, TrafficScope};
use sha2::{Digest, Sha256};

use crate::hex::encode_lower_hex;
use crate::subject_tokens;

const MAX_STREAM_NAME_LEN: usize = 255;
pub const MAX_SUPPORTED_EVENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_ATOMIC_HEADER_BYTES: usize = 4 * 1024;
const DUPLICATE_WINDOW: Duration = Duration::from_mins(2);
pub const DEFAULT_EVENT_STORE_MAX_STREAM_BYTES: i64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_EVENT_STORE_MAX_EVENT_BYTES: usize = 512 * 1024;
pub const DEFAULT_EVENT_STORE_REPLICAS: usize = 1;
pub const DEFAULT_EVENT_STORE_PUBACK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EventByteLimit {
    value: usize,
    wire_value: usize,
    nats_value: i32,
    comparison_value: i64,
}

impl EventByteLimit {
    const DEFAULT: Self = Self {
        value: DEFAULT_EVENT_STORE_MAX_EVENT_BYTES,
        wire_value: 516 * 1024,
        nats_value: 516 * 1024,
        comparison_value: 516 * 1024,
    };

    fn new(value: usize) -> Result<Self, EventStoreError> {
        if value == 0 || value > MAX_SUPPORTED_EVENT_BYTES {
            return Err(invalid(format!(
                "maximum event bytes must be between 1 and {MAX_SUPPORTED_EVENT_BYTES}"
            )));
        }
        let wire_value = value
            .checked_add(MAX_ATOMIC_HEADER_BYTES)
            .ok_or_else(|| invalid("maximum wire message bytes overflowed"))?;
        let nats_value = i32::try_from(wire_value).map_err(|_| {
            invalid("maximum wire message bytes cannot be represented by JetStream")
        })?;
        let comparison_value = i64::try_from(wire_value).map_err(|_| {
            invalid("maximum wire message bytes cannot be represented by JetStream")
        })?;
        Ok(Self {
            value,
            wire_value,
            nats_value,
            comparison_value,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsEventStoreConfig {
    completed_outcome_epoch: Option<String>,
    split_outcomes: bool,
    explicit_detail_expiry: bool,
    read_only_outcomes: bool,
    legacy_security_scope: Option<rostfrei_core::SecurityScope>,
    outcome_state_stream_name: String,
    application: ApplicationName,
    traffic_scope: TrafficScope,
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
            scoped_subject_prefix(context.application().as_str(), context.traffic_scope()),
            context.name().as_str()
        );
        let config = Self {
            completed_outcome_epoch: None,
            split_outcomes: false,
            explicit_detail_expiry: false,
            read_only_outcomes: false,
            legacy_security_scope: None,
            outcome_state_stream_name: String::new(),
            application: context.application().clone(),
            traffic_scope: context.traffic_scope(),
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

    pub const fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope
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

    /// Operator assertion: this is a NEW isolated authority namespace, with old writers
    /// excluded by broker credentials/ACLs. Never use to activate an existing legacy store.
    /// Provisioning durably binds this epoch at stream creation. The default legacy
    /// profile permits ordinary event writes but blocks unknown command outcomes.
    pub fn with_fresh_completed_outcome_epoch(
        mut self,
        epoch: impl Into<String>,
    ) -> Result<Self, EventStoreError> {
        let epoch = epoch.into();
        if epoch.is_empty()
            || epoch.len() > 128
            || !epoch
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid("invalid completed outcome identity epoch"));
        }
        self.completed_outcome_epoch = Some(epoch);
        Ok(self)
    }

    pub fn completed_outcome_epoch(&self) -> Option<&str> {
        self.completed_outcome_epoch.as_deref()
    }

    /// Explicit fenced cutover. Derives disjoint journal and state subjects; original
    /// identities stay unchanged and old outcomes must be routed to their old authority.
    pub fn with_fresh_split_outcome_epoch(
        self,
        epoch: impl Into<String>,
    ) -> Result<Self, EventStoreError> {
        if self.split_outcomes {
            return Err(invalid("split authority already selected"));
        }
        let mut config = self.with_fresh_completed_outcome_epoch(epoch)?;
        let epoch = config
            .completed_outcome_epoch
            .as_deref()
            .ok_or_else(|| invalid("missing epoch"))?;
        let token = digest_parts(&[epoch.as_bytes()]);
        config.stream_name = format!("{}__SPLIT_{}", config.stream_name, token);
        config.subject_prefix = format!("{}.split.{token}", config.subject_prefix);
        config.outcome_state_stream_name = format!("{}_STATE", config.stream_name);
        config.split_outcomes = true;
        config.validate()?;
        if !valid_stream_name(&config.outcome_state_stream_name) {
            return Err(invalid("state stream name is too long"));
        }
        Ok(config)
    }

    pub fn with_explicit_detail_expiry(mut self, enabled: bool) -> Result<Self, EventStoreError> {
        if enabled && !self.split_outcomes {
            return Err(invalid("inline outcomes do not support expiry"));
        }
        self.explicit_detail_expiry = enabled;
        Ok(self)
    }

    #[must_use]
    pub fn with_legacy_security_scope(mut self, scope: rostfrei_core::SecurityScope) -> Self {
        self.legacy_security_scope = Some(scope);
        self
    }
    pub(crate) const fn legacy_security_scope(&self) -> Option<&rostfrei_core::SecurityScope> {
        self.legacy_security_scope.as_ref()
    }
    pub(crate) const fn split_outcomes(&self) -> bool {
        self.split_outcomes
    }
    pub(crate) const fn explicit_detail_expiry(&self) -> bool {
        self.explicit_detail_expiry
    }
    /// Connect a replay-only handle: all persistence mutations are refused, including
    /// legacy event appends, append sessions, completed/import writes and detail expiry.
    /// History and known-outcome reads remain available. Provisioning helpers are
    /// separate explicit operations; this setting grants no provisioning authority.
    #[must_use]
    pub const fn with_read_only_outcomes(mut self) -> Self {
        self.read_only_outcomes = true;
        self
    }
    pub(crate) const fn read_only_outcomes(&self) -> bool {
        self.read_only_outcomes
    }
    pub fn outcome_state_stream_name(&self) -> &str {
        &self.outcome_state_stream_name
    }
    pub(crate) fn outcome_state_subject(&self, manifest: &[u8]) -> String {
        format!(
            "{}.outcome.{}",
            self.subject_prefix,
            digest_parts(&[manifest])
        )
    }

    pub(crate) fn outcome_state_config(&self) -> Config {
        let mut config = self.stream_config();
        config.name.clone_from(&self.outcome_state_stream_name);
        config.subjects = vec![format!("{}.outcome.*", self.subject_prefix)];
        config.max_messages_per_subject = 1;
        config.discard_new_per_subject = false;
        config.allow_atomic_publish = false;
        config
            .metadata
            .insert("rostfrei.outcome-state.version".to_owned(), "1".to_owned());
        config
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

    pub(crate) const fn max_wire_message_bytes(&self) -> usize {
        self.max_event_bytes.wire_value
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

    pub fn event_subject(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
        correlation: Option<&str>,
        causation: Option<&str>,
    ) -> String {
        format!(
            "{}.aggregate.{}.{}.{}.{}",
            self.subject_prefix,
            subject_tokens::encode(aggregate_type),
            subject_tokens::encode(aggregate_id),
            subject_tokens::optional(correlation),
            subject_tokens::optional(causation),
        )
    }

    /// Used for both replay and native aggregate-wide concurrency checks.
    pub fn aggregate_history_subject_filter(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> String {
        format!(
            "{}.aggregate.{}.{}.*.*",
            self.subject_prefix,
            subject_tokens::encode(aggregate_type),
            subject_tokens::encode(aggregate_id),
        )
    }

    pub fn correlation_subject_filter(&self, correlation: &str) -> String {
        format!(
            "{}.aggregate.*.*.{}.*",
            self.subject_prefix,
            subject_tokens::encode(correlation)
        )
    }

    pub fn causation_subject_filter(&self, causation: &str) -> String {
        format!(
            "{}.aggregate.*.*.*.{}",
            self.subject_prefix,
            subject_tokens::encode(causation)
        )
    }

    pub fn aggregate_subject_filter(&self) -> String {
        format!("{}.aggregate.*.*.*.*", self.subject_prefix)
    }

    pub fn transaction_subject(&self, operation_id: &str) -> String {
        let digest = digest_parts(&[operation_id.as_bytes()]);
        format!("{}.transaction.{digest}", self.subject_prefix)
    }

    pub fn transaction_guard_subject(&self, operation_id: &str, ordinal: usize) -> String {
        format!("{}.guard.{ordinal}", self.transaction_subject(operation_id))
    }

    pub(crate) fn legacy_primary_transaction_subject(
        &self,
        primary_stream_id: &rostfrei_core::StreamId,
        operation_id: &str,
    ) -> String {
        let digest = digest_parts(&[
            primary_stream_id.aggregate_type().as_str().as_bytes(),
            primary_stream_id.aggregate_id().as_str().as_bytes(),
            operation_id.as_bytes(),
        ]);
        format!("{}.transaction.{digest}", self.subject_prefix)
    }

    pub(crate) fn legacy_primary_transaction_guard_subject(
        &self,
        primary_stream_id: &rostfrei_core::StreamId,
        operation_id: &str,
        ordinal: usize,
    ) -> String {
        format!(
            "{}.guard.{ordinal}",
            self.legacy_primary_transaction_subject(primary_stream_id, operation_id)
        )
    }

    pub fn transaction_subject_filter(&self) -> String {
        format!("{}.transaction.>", self.subject_prefix)
    }

    pub fn stream_config(&self) -> Config {
        Config {
            name: self.stream_name.clone(),
            subjects: vec![
                self.aggregate_subject_filter(),
                self.transaction_subject_filter(),
            ],
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
            metadata: self
                .completed_outcome_epoch
                .as_ref()
                .map(|epoch| {
                    std::collections::HashMap::from([
                        (
                            "rostfrei.completed-outcomes.epoch".to_owned(),
                            epoch.clone(),
                        ),
                        (
                            "rostfrei.completed-outcomes.version".to_owned(),
                            if self.split_outcomes { "2" } else { "1" }.to_owned(),
                        ),
                    ])
                })
                .unwrap_or_default(),
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
                "maximum stream bytes must be at least maximum wire message bytes",
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
        scoped_stream_prefix(context.application().as_str(), context.traffic_scope()),
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

fn scoped_subject_prefix(application: &str, traffic_scope: TrafficScope) -> String {
    traffic_scope.subject_segment().map_or_else(
        || application.to_owned(),
        |scope| format!("{application}.{scope}"),
    )
}

fn scoped_stream_prefix(application: &str, traffic_scope: TrafficScope) -> String {
    let application = stream_token(application);
    match traffic_scope {
        TrafficScope::Normal => application,
        TrafficScope::Test => format!("{application}__TEST"),
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
            "fast-inbox.domain.commercial-access.aggregate.*.*.*.*"
        );
        assert_eq!(config.subject_filter(), config.aggregate_subject_filter());
        assert_eq!(
            config.stream_config().subjects,
            vec![
                "fast-inbox.domain.commercial-access.aggregate.*.*.*.*",
                "fast-inbox.domain.commercial-access.transaction.>",
            ]
        );
    }

    #[test]
    fn test_bounded_context_derives_disjoint_event_store_resources() {
        let context = ApplicationName::new("fast-inbox")
            .unwrap()
            .test_bounded_context("commercial-access")
            .unwrap();
        let config = NatsEventStoreConfig::for_bounded_context(&context).unwrap();

        assert_eq!(config.traffic_scope(), TrafficScope::Test);
        assert_eq!(
            config.stream_name(),
            "FAST_INBOX__TEST__COMMERCIAL_ACCESS_DOMAIN_EVENTS"
        );
        assert_eq!(
            config.subject_prefix(),
            "fast-inbox.test.domain.commercial-access"
        );
        assert_eq!(
            config.aggregate_subject_filter(),
            "fast-inbox.test.domain.commercial-access.aggregate.*.*.*.*"
        );
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
        assert_eq!(config.max_message_size, 1024 * 1024 + 4 * 1024);
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
        assert_eq!(config.max_event_bytes(), 512 * 1024);
        assert_eq!(config.max_wire_message_bytes(), 516 * 1024);
        assert_eq!(config.stream_config().max_message_size, 516 * 1024);
        assert_eq!(config.replicas(), DEFAULT_EVENT_STORE_REPLICAS);
        assert_eq!(config.puback_timeout(), DEFAULT_EVENT_STORE_PUBACK_TIMEOUT);
    }

    #[test]
    fn configured_event_limit_supports_the_historical_global_maximum() {
        let unsupported = MAX_SUPPORTED_EVENT_BYTES.checked_add(1).unwrap();
        let maximum_wire_bytes = MAX_SUPPORTED_EVENT_BYTES.checked_add(4 * 1024).unwrap();
        let config = NatsEventStoreConfig::for_bounded_context(&context())
            .unwrap()
            .with_storage_limits(
                i64::try_from(maximum_wire_bytes).unwrap(),
                MAX_SUPPORTED_EVENT_BYTES,
            )
            .unwrap();

        assert_eq!(config.max_event_bytes(), MAX_SUPPORTED_EVENT_BYTES);
        assert!(
            NatsEventStoreConfig::for_bounded_context(&context())
                .unwrap()
                .with_storage_limits(i64::try_from(unsupported).unwrap(), unsupported,)
                .is_err()
        );
    }

    #[test]
    fn stream_capacity_must_cover_the_wire_message_allowance() {
        let payload_bytes = 512 * 1024;
        let payload_only = i64::try_from(payload_bytes).unwrap();
        let wire_bytes = i64::try_from(payload_bytes + 4 * 1024).unwrap();

        let error = NatsEventStoreConfig::for_bounded_context(&context())
            .unwrap()
            .with_storage_limits(payload_only, payload_bytes)
            .expect_err("payload-only stream capacity must be rejected");
        assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
        assert!(error.message().contains("wire message"));

        NatsEventStoreConfig::for_bounded_context(&context())
            .unwrap()
            .with_storage_limits(wire_bytes, payload_bytes)
            .expect("wire-sized stream capacity must be accepted");
    }

    #[test]
    fn event_subject_and_filters_preserve_separate_readable_identity_tokens() {
        let config =
            NatsEventStoreConfig::new(&context(), "EVENT_STORE_TEST").expect("valid config");
        let subject = config.event_subject("Account", "account-123", Some("flow.1"), None);
        assert_eq!(
            subject,
            config.event_subject("Account", "account-123", Some("flow.1"), None)
        );
        assert_eq!(
            subject,
            "fast-inbox.domain.commercial-access.aggregate.Account.account-123.flow%2E1.~"
        );
        assert_eq!(
            config.aggregate_history_subject_filter("Account", "account-123"),
            "fast-inbox.domain.commercial-access.aggregate.Account.account-123.*.*"
        );
        assert_eq!(
            config.correlation_subject_filter("flow.1"),
            "fast-inbox.domain.commercial-access.aggregate.*.*.flow%2E1.*"
        );
        assert_eq!(
            config.causation_subject_filter("cause.*"),
            "fast-inbox.domain.commercial-access.aggregate.*.*.*.cause%2E%2A"
        );
    }

    #[test]
    fn transaction_subject_is_operation_scoped_and_opaque() {
        let config =
            NatsEventStoreConfig::new(&context(), "EVENT_STORE_TEST").expect("valid config");
        let subject = config.transaction_subject("operation-123");

        assert_eq!(subject, config.transaction_subject("operation-123"));
        assert!(subject.starts_with("fast-inbox.domain.commercial-access.transaction."));
        assert!(!subject.contains("operation-123"));
        assert_ne!(subject, config.transaction_subject("operation-124"));
        assert_eq!(
            config.transaction_guard_subject("operation-123", 2),
            format!("{subject}.guard.2")
        );

        let primary = rostfrei_core::StreamId::new(
            rostfrei_core::AggregateType::new("Account").unwrap(),
            rostfrei_core::AggregateId::new("account-123").unwrap(),
        );
        let legacy = config.legacy_primary_transaction_subject(&primary, "operation-123");
        assert_ne!(subject, legacy);
        assert_eq!(
            config.legacy_primary_transaction_guard_subject(&primary, "operation-123", 2),
            format!("{legacy}.guard.2")
        );
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
