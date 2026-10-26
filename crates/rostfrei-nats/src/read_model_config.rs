use std::time::Duration;

use async_nats::jetstream::{
    self,
    stream::{Config, DiscardPolicy, StorageType},
};
use rostfrei_core::{ReadModelError, ReadModelErrorKind};
use rostfrei_messaging_core::{ApplicationName, BoundedContext, BoundedContextName, TrafficScope};

use crate::{
    StreamStorage,
    messaging_config::traffic_stream_prefix,
    stream_policy::{is_stream_not_found, stream_config_mismatches},
};

const HEADER_ALLOWANCE: i32 = 1024;
const DEFAULT_MAX_BYTES: i64 = 64 * 1024 * 1024;
const DEFAULT_MAX_VALUE_BYTES: i32 = 256 * 1024;

/// Operator-owned bucket policy. Construction and adapter connection never
/// provision resources. TTL is retention, not an application freshness deadline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NatsReadModelConfig {
    application: ApplicationName,
    traffic_scope: TrafficScope,
    context: Option<BoundedContext>,
    name: BoundedContextName,
    bucket: String,
    max_bytes: i64,
    max_value_bytes: i32,
    storage: StreamStorage,
    replicas: usize,
    history: i64,
    ttl: Duration,
}

impl NatsReadModelConfig {
    pub fn for_model<M: rostfrei_core::ReadModel>(
        context: &BoundedContext,
    ) -> Result<Self, ReadModelError> {
        Self::new(context, M::NAME)
    }

    pub fn new(context: &BoundedContext, name: impl Into<String>) -> Result<Self, ReadModelError> {
        let name = model_name(name)?;
        let bucket = format!(
            "{}__{}__{}_READ_MODEL",
            traffic_stream_prefix(context.application().as_str(), context.traffic_scope()),
            traffic_stream_prefix(context.name().as_str(), TrafficScope::Normal),
            traffic_stream_prefix(name.as_str(), TrafficScope::Normal)
        );
        Ok(Self::with_bucket(
            context.application().clone(),
            context.traffic_scope(),
            Some(context.clone()),
            name,
            bucket,
        ))
    }

    /// A framework-owned model shared by every bounded context of an application.
    /// The bucket name omits the context segment and keeps the traffic scope.
    pub fn application_scoped(
        application: &ApplicationName,
        traffic_scope: TrafficScope,
        name: impl Into<String>,
    ) -> Result<Self, ReadModelError> {
        let name = model_name(name)?;
        let bucket = format!(
            "{}__{}_READ_MODEL",
            traffic_stream_prefix(application.as_str(), traffic_scope),
            traffic_stream_prefix(name.as_str(), TrafficScope::Normal)
        );
        Ok(Self::with_bucket(
            application.clone(),
            traffic_scope,
            None,
            name,
            bucket,
        ))
    }

    const fn with_bucket(
        application: ApplicationName,
        traffic_scope: TrafficScope,
        context: Option<BoundedContext>,
        name: BoundedContextName,
        bucket: String,
    ) -> Self {
        Self {
            application,
            traffic_scope,
            context,
            name,
            bucket,
            max_bytes: DEFAULT_MAX_BYTES,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
            storage: StreamStorage::File,
            replicas: 1,
            history: 1,
            ttl: Duration::ZERO,
        }
    }

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub const fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope
    }

    /// The owning bounded context, or `None` for an application-scoped model.
    pub const fn context(&self) -> Option<&BoundedContext> {
        self.context.as_ref()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn bucket_name(&self) -> &str {
        &self.bucket
    }

    pub fn stream_name(&self) -> String {
        format!("KV_{}", self.bucket)
    }

    pub const fn max_bytes(&self) -> i64 {
        self.max_bytes
    }

    /// Maximum encoded value bytes, including the schema envelope. NATS header
    /// space is reserved separately in the underlying stream policy.
    pub const fn max_value_bytes(&self) -> i32 {
        self.max_value_bytes
    }

    pub const fn storage(&self) -> StreamStorage {
        self.storage
    }

    pub const fn replicas(&self) -> usize {
        self.replicas
    }

    pub const fn history(&self) -> i64 {
        self.history
    }

    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn with_storage_limits(
        mut self,
        max_bytes: i64,
        max_value_bytes: i32,
    ) -> Result<Self, ReadModelError> {
        let wire_limit = max_value_bytes
            .checked_add(HEADER_ALLOWANCE)
            .ok_or_else(|| invalid("value limit overflow"))?;
        if max_value_bytes <= 0 || max_bytes < i64::from(wire_limit) {
            return Err(invalid(
                "bucket capacity must hold a positive value limit plus header allowance",
            ));
        }
        self.max_bytes = max_bytes;
        self.max_value_bytes = max_value_bytes;
        Ok(self)
    }

    #[must_use]
    pub const fn with_storage(mut self, storage: StreamStorage) -> Self {
        self.storage = storage;
        self
    }

    pub fn with_replicas(mut self, replicas: usize) -> Result<Self, ReadModelError> {
        if !(1..=5).contains(&replicas) {
            return Err(invalid("replicas must be between 1 and 5"));
        }
        self.replicas = replicas;
        Ok(self)
    }

    pub fn with_history(mut self, history: i64) -> Result<Self, ReadModelError> {
        if !(1..=64).contains(&history) {
            return Err(invalid("KV history must be between 1 and 64"));
        }
        self.history = history;
        Ok(self)
    }

    pub fn with_ttl(mut self, ttl: Option<Duration>) -> Result<Self, ReadModelError> {
        if ttl.is_some_and(|ttl| ttl.is_zero() || ttl.as_nanos() > i64::MAX.unsigned_abs().into()) {
            return Err(invalid("TTL must be positive and fit NATS nanoseconds"));
        }
        self.ttl = ttl.unwrap_or_default();
        Ok(self)
    }

    pub(crate) fn stream_config(&self) -> Config {
        Config {
            name: self.stream_name(),
            description: Some("Rostfrei application-owned read model".to_owned()),
            subjects: vec![format!("$KV.{}.>", self.bucket)],
            max_bytes: self.max_bytes,
            max_messages: -1,
            max_consumers: -1,
            max_messages_per_subject: self.history,
            max_message_size: self.max_value_bytes.saturating_add(HEADER_ALLOWANCE),
            max_age: self.ttl,
            storage: match self.storage {
                StreamStorage::File => StorageType::File,
                StreamStorage::Memory => StorageType::Memory,
            },
            num_replicas: self.replicas,
            discard: DiscardPolicy::New,
            allow_rollup: true,
            deny_delete: true,
            allow_direct: false,
            duplicate_window: if self.ttl.is_zero() {
                Duration::from_secs(120)
            } else {
                self.ttl.min(Duration::from_secs(120))
            },
            ..Default::default()
        }
    }
}

/// Creates a missing bucket or verifies an existing one without changing policy.
pub async fn provision_read_model(
    context: &jetstream::Context,
    config: &NatsReadModelConfig,
) -> Result<(), ReadModelError> {
    match context.get_stream(config.stream_name()).await {
        Ok(stream) => verify_config(config, &stream.cached_info().config),
        Err(error) if is_stream_not_found(&error) => {
            let created = context.create_stream(config.stream_config()).await;
            match created {
                Ok(stream) => verify_config(config, &stream.cached_info().config),
                Err(_) => verify_read_model(context, config).await,
            }
        }
        Err(_) => Err(unavailable()),
    }
}

/// Verifies all relevant stream policy, routing, and KV consistency settings.
pub async fn verify_read_model(
    context: &jetstream::Context,
    config: &NatsReadModelConfig,
) -> Result<(), ReadModelError> {
    let stream = context
        .get_stream(config.stream_name())
        .await
        .map_err(|_| unavailable())?;
    verify_config(config, &stream.cached_info().config)
}

/// Explicit operator policy update. Requires an existing correctly scoped KV
/// bucket; lowering capacity/history/TTL may remove retained values.
pub async fn update_read_model(
    context: &jetstream::Context,
    config: &NatsReadModelConfig,
) -> Result<(), ReadModelError> {
    let stream = context
        .get_stream(config.stream_name())
        .await
        .map_err(|_| unavailable())?;
    let actual = &stream.cached_info().config;
    let mut expected = config.stream_config();
    expected.max_bytes = actual.max_bytes;
    expected.max_message_size = actual.max_message_size;
    expected.max_messages_per_subject = actual.max_messages_per_subject;
    expected.max_age = actual.max_age;
    expected.duplicate_window = actual.duplicate_window;
    expected.storage = actual.storage;
    expected.num_replicas = actual.num_replicas;
    verify_expected(&expected, actual)?;
    let updated = context
        .update_stream(config.stream_config())
        .await
        .map_err(|_| unavailable())?;
    verify_config(config, &updated.config)
}

pub fn verify_config(config: &NatsReadModelConfig, actual: &Config) -> Result<(), ReadModelError> {
    verify_expected(&config.stream_config(), actual)
}

fn verify_expected(expected: &Config, actual: &Config) -> Result<(), ReadModelError> {
    let mismatches = stream_config_mismatches(expected, actual);
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(ReadModelError::new(
            ReadModelErrorKind::ConfigurationMismatch,
            format!(
                "read-model bucket policy differs: {}",
                mismatches.join(", ")
            ),
        ))
    }
}

fn model_name(name: impl Into<String>) -> Result<BoundedContextName, ReadModelError> {
    BoundedContextName::new(name)
        .map_err(|_| invalid("read-model name must be a lowercase kebab-case scope name"))
}

fn invalid(message: &str) -> ReadModelError {
    ReadModelError::new(ReadModelErrorKind::InvalidRequest, message)
}

pub fn unavailable() -> ReadModelError {
    ReadModelError::new(
        ReadModelErrorKind::Unavailable,
        "read-model storage unavailable; write outcome may be uncertain",
    )
}
