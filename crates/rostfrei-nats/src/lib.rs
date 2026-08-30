//! NATS adapters for rostfrei event sourcing and messaging.

mod command_response;
mod connection;
mod consumer;
mod domain_event_consumer;
mod error;
mod event_store;
mod event_store_config;
mod hex;
mod messaging_adapter;
mod messaging_config;
mod provisioning;
mod publish;
mod query;
mod stream_policy;

pub use command_response::{
    DEFAULT_COMMAND_RESPONSE_POLL_INTERVAL, MAX_COMMAND_RESPONSE_POLL_INTERVAL,
    NatsCommandResponseReader,
};
pub use connection::{ConnectionHealth, NatsConnection, connect};
pub use consumer::{MAX_QUARANTINE_RECORD_BYTES, NatsConsumerFactory, QuarantineRecord};
pub use domain_event_consumer::{
    DomainEventConsumerError, DomainEventConsumerErrorKind, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, provision_domain_event_consumer,
};
pub use error::{NatsError, NatsErrorKind};
pub use event_store::{NatsEventStore, provision_event_store};
pub use event_store_config::{
    DEFAULT_EVENT_STORE_MAX_EVENT_BYTES, DEFAULT_EVENT_STORE_MAX_STREAM_BYTES,
    DEFAULT_EVENT_STORE_PUBACK_TIMEOUT, DEFAULT_EVENT_STORE_REPLICAS, NatsEventStoreConfig,
};
pub use messaging_adapter::{NatsCommandHandler, NatsMessagingAdapter};
pub use messaging_config::{
    MINIMUM_NATS_SERVER_VERSION, MessagingTopology, NatsConnectionConfig, QueueGroup,
    ServerVersion, StreamName, SubjectFilter,
};
pub use provisioning::{
    ApplicationMessagingConfig, StreamProvisioningConfig, StreamRetention, StreamStorage,
    provision_application_messaging, provision_durable_consumer, provision_stream,
    verify_application_messaging, verify_stream,
};
pub use publish::{NatsPublishAck, NatsPublisher};
pub use query::{NatsQueryRequester, NatsQueryServer, NatsQueryServerConfig};
