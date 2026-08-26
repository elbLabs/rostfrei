//! NATS adapters for rostfrei event sourcing and messaging.

mod connection;
mod consumer;
mod domain_event_consumer;
mod error;
mod event_store;
mod event_store_config;
mod messaging_config;
mod provisioning;
mod publish;
mod query;

pub use connection::{connect, ConnectionHealth, NatsConnection};
pub use consumer::{NatsConsumerFactory, QuarantineRecord, MAX_QUARANTINE_RECORD_BYTES};
pub use domain_event_consumer::{
    provision_domain_event_consumer, DomainEventConsumerError, DomainEventConsumerErrorKind,
    NatsDomainEventConsumer, NatsDomainEventConsumerConfig,
};
pub use error::{NatsError, NatsErrorKind};
pub use event_store::{provision_event_store, NatsEventStore};
pub use event_store_config::NatsEventStoreConfig;
pub use messaging_config::{
    MessagingTopology, NatsConnectionConfig, QueueGroup, ServerVersion, StreamName, SubjectFilter,
    MINIMUM_NATS_SERVER_VERSION,
};
pub use provisioning::{
    provision_durable_consumer, provision_stream, verify_stream, StreamProvisioningConfig,
    StreamRetention, StreamStorage,
};
pub use publish::{NatsPublishAck, NatsPublisher};
pub use query::{NatsQueryRequester, NatsQueryServer, NatsQueryServerConfig};
