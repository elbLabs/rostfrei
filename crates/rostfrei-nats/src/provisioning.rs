use std::time::Duration;

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    stream::{self, DiscardPolicy, RetentionPolicy, StorageType},
};
use rostfrei_messaging_core::{
    ApplicationName, ConsumerConfig, MAX_MESSAGE_PAYLOAD_BYTES, PublishableAddress,
};

use crate::{
    error::NatsError,
    messaging_config::{MessagingTopology, StreamName, SubjectFilter},
    stream_policy::{is_stream_not_found, stream_config_mismatches},
};

pub const DEFAULT_STREAM_MAX_BYTES: i64 = 10 * 1024 * 1024 * 1024;
pub const DEFAULT_STREAM_MAX_AGE: Duration = Duration::from_hours(720);
pub const DEFAULT_STREAM_MAX_MESSAGE_BYTES: i32 = 2 * 1024 * 1024;
pub const DEFAULT_DUPLICATE_WINDOW: Duration = Duration::from_mins(2);
const SOURCE_STREAM_MESSAGE_OVERHEAD_BYTES: usize = 64 * 1024;

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
    application: ApplicationName,
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
        application: &ApplicationName,
        name: StreamName,
        subjects: Vec<SubjectFilter>,
        retention: StreamRetention,
    ) -> Result<Self, NatsError> {
        let config = Self {
            application: application.clone(),
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
            || !subjects_belong_to_application(&self.application, &self.subjects)
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

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub fn subjects(&self) -> &[SubjectFilter] {
        &self.subjects
    }

    pub const fn retention(&self) -> StreamRetention {
        self.retention
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
            max_messages: -1,
            max_messages_per_subject: -1,
            max_bytes: self.max_bytes,
            max_age: self.max_age,
            max_message_size: self.maximum_message_bytes,
            max_consumers: -1,
            duplicate_window: self.duplicate_window,
            num_replicas: self.replicas,
            no_ack: false,
            ..Default::default()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationMessagingConfig {
    topology: MessagingTopology,
    commands: StreamProvisioningConfig,
    command_responses: StreamProvisioningConfig,
    integration_events: StreamProvisioningConfig,
    quarantine: StreamProvisioningConfig,
}

impl ApplicationMessagingConfig {
    pub fn new(application: &ApplicationName) -> Result<Self, NatsError> {
        let topology = MessagingTopology::for_application(application)?;
        let source_maximum_message_bytes =
            i32::try_from(MAX_MESSAGE_PAYLOAD_BYTES + SOURCE_STREAM_MESSAGE_OVERHEAD_BYTES)
                .map_err(|_| NatsError::Configuration)?;
        let commands = StreamProvisioningConfig::new(
            application,
            topology.command_stream().clone(),
            vec![application_subject_filter(application, "command")?],
            StreamRetention::WorkQueue,
        )?
        .with_description(format!("{} commands", application.as_str()))
        .with_maximum_message_bytes(source_maximum_message_bytes);
        let command_responses = StreamProvisioningConfig::new(
            application,
            topology.command_response_stream().clone(),
            vec![application_subject_filter(application, "command-response")?],
            StreamRetention::Limits,
        )?
        .with_description(format!("{} command responses", application.as_str()))
        .with_maximum_message_bytes(source_maximum_message_bytes);
        let integration_events = StreamProvisioningConfig::new(
            application,
            topology.integration_event_stream().clone(),
            vec![application_subject_filter(application, "integration")?],
            StreamRetention::Limits,
        )?
        .with_description(format!("{} integration events", application.as_str()))
        .with_maximum_message_bytes(source_maximum_message_bytes);
        let quarantine = StreamProvisioningConfig::new(
            application,
            topology.quarantine_stream().clone(),
            vec![application_subject_filter(application, "quarantine")?],
            StreamRetention::Limits,
        )?
        .with_description(format!("{} quarantined messages", application.as_str()));
        Ok(Self {
            topology,
            commands,
            command_responses,
            integration_events,
            quarantine,
        })
    }

    pub fn with_replicas(mut self, replicas: usize) -> Result<Self, NatsError> {
        self.commands = self.commands.with_replicas(replicas);
        self.command_responses = self.command_responses.with_replicas(replicas);
        self.integration_events = self.integration_events.with_replicas(replicas);
        self.quarantine = self.quarantine.with_replicas(replicas);
        for stream in self.streams() {
            stream.validate()?;
        }
        Ok(self)
    }

    pub fn with_max_bytes(mut self, max_bytes: i64) -> Result<Self, NatsError> {
        self.commands = self.commands.with_max_bytes(max_bytes);
        self.command_responses = self.command_responses.with_max_bytes(max_bytes);
        self.integration_events = self.integration_events.with_max_bytes(max_bytes);
        self.quarantine = self.quarantine.with_max_bytes(max_bytes);
        for stream in self.streams() {
            stream.validate()?;
        }
        Ok(self)
    }

    pub const fn application(&self) -> &ApplicationName {
        self.topology.application()
    }

    pub const fn topology(&self) -> &MessagingTopology {
        &self.topology
    }

    pub const fn commands(&self) -> &StreamProvisioningConfig {
        &self.commands
    }

    pub const fn integration_events(&self) -> &StreamProvisioningConfig {
        &self.integration_events
    }

    pub const fn command_responses(&self) -> &StreamProvisioningConfig {
        &self.command_responses
    }

    pub const fn quarantine(&self) -> &StreamProvisioningConfig {
        &self.quarantine
    }

    pub const fn streams(&self) -> [&StreamProvisioningConfig; 4] {
        [
            &self.commands,
            &self.command_responses,
            &self.integration_events,
            &self.quarantine,
        ]
    }
}

fn application_subject_filter(
    application: &ApplicationName,
    kind: &str,
) -> Result<SubjectFilter, NatsError> {
    SubjectFilter::new(format!("{}.{kind}.>", application.as_str()))
}

fn subjects_belong_to_application(
    application: &ApplicationName,
    subjects: &[SubjectFilter],
) -> bool {
    !subjects.is_empty()
        && subjects
            .iter()
            .all(|subject| subject.as_str().split('.').next() == Some(application.as_str()))
}

pub async fn provision_stream(
    context: &jetstream::Context,
    config: &StreamProvisioningConfig,
) -> Result<stream::Info, NatsError> {
    match context.get_stream(config.name().as_str()).await {
        Ok(existing) => {
            let subjects = existing
                .cached_info()
                .config
                .subjects
                .iter()
                .map(|subject| SubjectFilter::new(subject.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            if !subjects_belong_to_application(config.application(), &subjects) {
                return Err(NatsError::Configuration);
            }
        }
        Err(error) if is_stream_not_found(&error) => {}
        Err(_) => return Err(NatsError::Provisioning),
    }
    let expected = config.as_nats_config()?;
    let provisioned = context
        .create_or_update_stream(expected.clone())
        .await
        .map_err(|_| NatsError::Provisioning)?;
    if !stream_config_mismatches(&expected, &provisioned.config).is_empty() {
        return Err(NatsError::Configuration);
    }
    Ok(provisioned)
}

pub async fn provision_application_messaging(
    context: &jetstream::Context,
    config: &ApplicationMessagingConfig,
) -> Result<(), NatsError> {
    for stream in config.streams() {
        provision_stream(context, stream).await?;
    }
    Ok(())
}

pub async fn verify_application_messaging(
    context: &jetstream::Context,
    config: &ApplicationMessagingConfig,
) -> Result<(), NatsError> {
    for expected in config.streams() {
        let stream = context
            .get_stream(expected.name().as_str())
            .await
            .map_err(|_| NatsError::StreamNotFound)?;
        let expected = expected.as_nats_config()?;
        let actual = &stream.cached_info().config;
        if !stream_config_mismatches(&expected, actual).is_empty() {
            return Err(NatsError::Configuration);
        }
    }
    Ok(())
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
        ack_wait: config.ack_wait(),
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
    if config.address().application() != topology.application().as_str() {
        return Err(NatsError::Configuration);
    }
    let stream_name = topology
        .stream_for(config.address().kind())
        .ok_or(NatsError::Configuration)?;
    let consumer = context
        .create_consumer_on_stream(durable_consumer_config(config)?, stream_name.as_str())
        .await
        .map_err(|_| NatsError::Provisioning)?;
    Ok(consumer.cached_info().clone())
}

#[cfg(test)]
mod tests {
    use rostfrei_messaging_core::{CommandAddress, ConsumerName, DurableName};

    use super::*;

    #[test]
    fn application_config_derives_disjoint_streams_and_subjects() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let config = ApplicationMessagingConfig::new(&application)
            .unwrap()
            .with_replicas(3)
            .unwrap();

        assert_eq!(config.application().as_str(), "fast-inbox");
        assert_eq!(config.commands.name().as_str(), "FAST_INBOX_COMMANDS");
        assert_eq!(
            config.command_responses.name().as_str(),
            "FAST_INBOX_COMMAND_RESPONSES"
        );
        assert_eq!(
            config.integration_events.name().as_str(),
            "FAST_INBOX_INTEGRATION_EVENTS"
        );
        assert_eq!(config.quarantine.name().as_str(), "FAST_INBOX_QUARANTINE");
        assert_eq!(
            config.commands.subjects()[0].as_str(),
            "fast-inbox.command.>"
        );
        assert_eq!(
            config.command_responses.subjects()[0].as_str(),
            "fast-inbox.command-response.>"
        );
        assert_eq!(
            config.integration_events.subjects()[0].as_str(),
            "fast-inbox.integration.>"
        );
        assert_eq!(
            config.quarantine.subjects()[0].as_str(),
            "fast-inbox.quarantine.>"
        );
        assert_eq!(config.commands.retention(), StreamRetention::WorkQueue);
        assert_eq!(
            config.command_responses.retention(),
            StreamRetention::Limits
        );
        assert_eq!(
            config.integration_events.retention(),
            StreamRetention::Limits
        );
        assert_eq!(config.quarantine.retention(), StreamRetention::Limits);
        assert_eq!(
            config.commands.maximum_message_bytes,
            i32::try_from(MAX_MESSAGE_PAYLOAD_BYTES + SOURCE_STREAM_MESSAGE_OVERHEAD_BYTES)
                .unwrap()
        );
        assert_eq!(
            config.command_responses.maximum_message_bytes,
            config.commands.maximum_message_bytes
        );
        assert_eq!(
            config.integration_events.maximum_message_bytes,
            config.commands.maximum_message_bytes
        );
        assert_eq!(
            config.quarantine.maximum_message_bytes,
            DEFAULT_STREAM_MAX_MESSAGE_BYTES
        );
        assert!(config.streams().iter().all(|stream| stream.replicas == 3));
    }

    #[test]
    fn application_config_rejects_invalid_replica_counts() {
        let application = ApplicationName::new("fast-inbox").unwrap();

        assert!(
            ApplicationMessagingConfig::new(&application)
                .unwrap()
                .with_replicas(0)
                .is_err()
        );
        assert!(
            ApplicationMessagingConfig::new(&application)
                .unwrap()
                .with_replicas(6)
                .is_err()
        );
    }

    #[test]
    fn application_config_validates_capacity_overrides() {
        let application = ApplicationName::new("fast-inbox").unwrap();

        assert!(
            ApplicationMessagingConfig::new(&application)
                .unwrap()
                .with_max_bytes(i64::from(DEFAULT_STREAM_MAX_MESSAGE_BYTES) - 1)
                .is_err()
        );
        let config = ApplicationMessagingConfig::new(&application)
            .unwrap()
            .with_max_bytes(64 * 1024 * 1024)
            .unwrap();
        assert!(
            config
                .streams()
                .iter()
                .all(|stream| stream.max_bytes == 64 * 1024 * 1024)
        );
    }

    #[test]
    fn durable_consumer_uses_the_configured_ack_wait() {
        let config = ConsumerConfig::new(
            ConsumerName::new("acme", "orders", "fulfillment", 1).unwrap(),
            DurableName::new("acme", "orders", "fulfillment", 1).unwrap(),
            CommandAddress::new("acme", "orders", "place-order").unwrap(),
            Duration::from_secs(45),
            Duration::from_secs(30),
            1,
            5,
        )
        .unwrap();

        let durable = durable_consumer_config(&config).unwrap();

        assert_eq!(durable.ack_wait, Duration::from_secs(45));
    }

    #[test]
    fn low_level_stream_config_rejects_cross_application_filters() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let name = StreamName::new("FAST_INBOX_CUSTOM").unwrap();

        for filter in [">", "*.command.>", "other.command.>"] {
            assert!(
                StreamProvisioningConfig::new(
                    &application,
                    name.clone(),
                    vec![SubjectFilter::new(filter).unwrap()],
                    StreamRetention::Limits,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn application_policy_comparison_covers_authoritative_controls() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let expected = ApplicationMessagingConfig::new(&application)
            .unwrap()
            .commands()
            .as_nats_config()
            .unwrap();

        for (field, actual) in [
            (
                "deny_delete",
                stream_config_with(&expected, |config| config.deny_delete = true),
            ),
            (
                "deny_purge",
                stream_config_with(&expected, |config| config.deny_purge = true),
            ),
            (
                "allow_rollup",
                stream_config_with(&expected, |config| config.allow_rollup = true),
            ),
            (
                "sealed",
                stream_config_with(&expected, |config| config.sealed = true),
            ),
        ] {
            assert!(stream_config_mismatches(&expected, &actual).contains(&field));
        }
    }

    fn stream_config_with(
        expected: &stream::Config,
        mutate: impl FnOnce(&mut stream::Config),
    ) -> stream::Config {
        let mut actual = expected.clone();
        mutate(&mut actual);
        actual
    }
}
