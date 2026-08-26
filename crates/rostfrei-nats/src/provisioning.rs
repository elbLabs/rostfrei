use std::time::Duration;

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    stream::{self, DiscardPolicy, RetentionPolicy, StorageType},
};
use rostfrei_messaging_core::{ConsumerConfig, PublishableAddress};

use crate::{
    error::NatsError,
    messaging_config::{MessagingTopology, StreamName, SubjectFilter},
};

pub const DEFAULT_STREAM_MAX_BYTES: i64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_STREAM_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const DEFAULT_STREAM_MAX_MESSAGE_BYTES: i32 = 2 * 1024 * 1024;
pub const DEFAULT_DUPLICATE_WINDOW: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamRetention {
    Limits,
    WorkQueue,
    Interest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamStorage {
    File,
    Memory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamProvisioningConfig {
    name: StreamName,
    subjects: Vec<SubjectFilter>,
    description: Option<String>,
    retention: StreamRetention,
    storage: StreamStorage,
    max_bytes: i64,
    max_age: Duration,
    maximum_message_bytes: i32,
    duplicate_window: Duration,
    replicas: usize,
}

impl StreamProvisioningConfig {
    pub fn new(
        name: StreamName,
        subjects: Vec<SubjectFilter>,
        retention: StreamRetention,
    ) -> Result<Self, NatsError> {
        let config = Self {
            name,
            subjects,
            description: None,
            retention,
            storage: StreamStorage::File,
            max_bytes: DEFAULT_STREAM_MAX_BYTES,
            max_age: DEFAULT_STREAM_MAX_AGE,
            maximum_message_bytes: DEFAULT_STREAM_MAX_MESSAGE_BYTES,
            duplicate_window: DEFAULT_DUPLICATE_WINDOW,
            replicas: 1,
        };
        config.validate()?;
        Ok(config)
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    #[must_use]
    pub const fn with_storage(mut self, storage: StreamStorage) -> Self {
        self.storage = storage;
        self
    }

    #[must_use]
    pub const fn with_max_bytes(mut self, max_bytes: i64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    #[must_use]
    pub const fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    #[must_use]
    pub const fn with_maximum_message_bytes(mut self, maximum_message_bytes: i32) -> Self {
        self.maximum_message_bytes = maximum_message_bytes;
        self
    }

    #[must_use]
    pub const fn with_duplicate_window(mut self, duplicate_window: Duration) -> Self {
        self.duplicate_window = duplicate_window;
        self
    }

    #[must_use]
    pub const fn with_replicas(mut self, replicas: usize) -> Self {
        self.replicas = replicas;
        self
    }

    pub fn validate(&self) -> Result<(), NatsError> {
        let description_is_invalid = self.description.as_ref().is_some_and(|description| {
            description.len() > 1024 || description.chars().any(char::is_control)
        });
        if self.subjects.is_empty()
            || description_is_invalid
            || self.maximum_message_bytes <= 0
            || self.max_bytes < i64::from(self.maximum_message_bytes)
            || self.max_age.is_zero()
            || self.duplicate_window.is_zero()
            || self.duplicate_window > self.max_age
            || !(1..=5).contains(&self.replicas)
        {
            return Err(NatsError::Configuration);
        }
        Ok(())
    }

    pub const fn name(&self) -> &StreamName {
        &self.name
    }

    pub fn subjects(&self) -> &[SubjectFilter] {
        &self.subjects
    }

    fn as_nats_config(&self) -> Result<stream::Config, NatsError> {
        self.validate()?;
        Ok(stream::Config {
            name: self.name.as_str().to_owned(),
            subjects: self
                .subjects
                .iter()
                .map(|subject| subject.as_str().to_owned())
                .collect(),
            description: self.description.clone(),
            retention: match self.retention {
                StreamRetention::Limits => RetentionPolicy::Limits,
                StreamRetention::WorkQueue => RetentionPolicy::WorkQueue,
                StreamRetention::Interest => RetentionPolicy::Interest,
            },
            storage: match self.storage {
                StreamStorage::File => StorageType::File,
                StreamStorage::Memory => StorageType::Memory,
            },
            discard: DiscardPolicy::New,
            max_bytes: self.max_bytes,
            max_age: self.max_age,
            max_message_size: self.maximum_message_bytes,
            duplicate_window: self.duplicate_window,
            num_replicas: self.replicas,
            no_ack: false,
            ..Default::default()
        })
    }
}

pub async fn provision_stream(
    context: &jetstream::Context,
    config: &StreamProvisioningConfig,
) -> Result<stream::Info, NatsError> {
    context
        .create_or_update_stream(config.as_nats_config()?)
        .await
        .map_err(|_| NatsError::Provisioning)
}

pub async fn verify_stream(
    context: &jetstream::Context,
    name: &StreamName,
) -> Result<(), NatsError> {
    context
        .get_stream(name.as_str())
        .await
        .map(|_| ())
        .map_err(|_| NatsError::StreamNotFound)
}

pub fn durable_consumer_config<A>(
    config: &ConsumerConfig<A>,
) -> Result<consumer::pull::Config, NatsError>
where
    A: PublishableAddress,
{
    let max_ack_pending =
        i64::try_from(config.concurrency()).map_err(|_| NatsError::Configuration)?;
    Ok(consumer::pull::Config {
        durable_name: Some(config.durable_name().as_str().to_owned()),
        name: Some(config.durable_name().as_str().to_owned()),
        description: Some(config.name().as_str().to_owned()),
        deliver_policy: DeliverPolicy::All,
        ack_policy: AckPolicy::Explicit,
        ack_wait: config.processing_timeout(),
        // The adapter enforces the bound so it can quarantine before terminating delivery.
        max_deliver: -1,
        filter_subject: config.address().as_str().to_owned(),
        max_ack_pending,
        max_batch: max_ack_pending,
        ..Default::default()
    })
}

pub async fn provision_durable_consumer<A>(
    context: &jetstream::Context,
    topology: &MessagingTopology,
    config: &ConsumerConfig<A>,
) -> Result<consumer::Info, NatsError>
where
    A: PublishableAddress,
{
    let stream_name = topology
        .stream_for(config.address().kind())
        .ok_or(NatsError::Configuration)?;
    let consumer = context
        .create_consumer_on_stream(durable_consumer_config(config)?, stream_name.as_str())
        .await
        .map_err(|_| NatsError::Provisioning)?;
    Ok(consumer.cached_info().clone())
}
