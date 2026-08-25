use thiserror::Error;

use crate::messaging_config::ServerVersion;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NatsErrorKind {
    Configuration,
    Connection,
    MinimumServerVersion,
    Serialization,
    PayloadTooLarge,
    PublishTimeout,
    PublishExpectation,
    StreamNotFound,
    ConsumerNotFound,
    Publish,
    Consumer,
    Acknowledgement,
    InvalidMessage,
    QueryTimeout,
    Query,
    Flush,
    Drain,
    Provisioning,
}

#[derive(Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum NatsError {
    #[error("NATS adapter configuration is invalid")]
    Configuration,
    #[error("NATS connection failed")]
    Connection,
    #[error("NATS server does not satisfy minimum version {required}")]
    MinimumServerVersion { required: ServerVersion },
    #[error("NATS message serialization failed")]
    Serialization,
    #[error("NATS message is too large: {actual} bytes exceeds {maximum} bytes")]
    PayloadTooLarge { actual: usize, maximum: usize },
    #[error("NATS message payload and headers exceed the server or stream limit")]
    MessageTooLarge,
    #[error("NATS publish acknowledgement timed out")]
    PublishTimeout,
    #[error("NATS publish expectation was not satisfied")]
    PublishExpectation,
    #[error("NATS stream was not found")]
    StreamNotFound,
    #[error("NATS durable consumer was not found")]
    ConsumerNotFound,
    #[error("NATS publish failed")]
    Publish,
    #[error("NATS consumer failed")]
    Consumer,
    #[error("NATS message acknowledgement failed")]
    Acknowledgement,
    #[error("NATS message is invalid")]
    InvalidMessage,
    #[error("NATS query timed out")]
    QueryTimeout,
    #[error("NATS query failed")]
    Query,
    #[error("NATS flush failed")]
    Flush,
    #[error("NATS drain failed")]
    Drain,
    #[error("NATS drain timed out")]
    DrainTimeout,
    #[error("NATS provisioning failed")]
    Provisioning,
}

impl NatsError {
    pub const fn kind(&self) -> NatsErrorKind {
        match self {
            Self::Configuration => NatsErrorKind::Configuration,
            Self::Connection => NatsErrorKind::Connection,
            Self::MinimumServerVersion { .. } => NatsErrorKind::MinimumServerVersion,
            Self::Serialization => NatsErrorKind::Serialization,
            Self::PayloadTooLarge { .. } | Self::MessageTooLarge => NatsErrorKind::PayloadTooLarge,
            Self::PublishTimeout => NatsErrorKind::PublishTimeout,
            Self::PublishExpectation => NatsErrorKind::PublishExpectation,
            Self::StreamNotFound => NatsErrorKind::StreamNotFound,
            Self::ConsumerNotFound => NatsErrorKind::ConsumerNotFound,
            Self::Publish => NatsErrorKind::Publish,
            Self::Consumer => NatsErrorKind::Consumer,
            Self::Acknowledgement => NatsErrorKind::Acknowledgement,
            Self::InvalidMessage => NatsErrorKind::InvalidMessage,
            Self::QueryTimeout => NatsErrorKind::QueryTimeout,
            Self::Query => NatsErrorKind::Query,
            Self::Flush => NatsErrorKind::Flush,
            Self::Drain | Self::DrainTimeout => NatsErrorKind::Drain,
            Self::Provisioning => NatsErrorKind::Provisioning,
        }
    }
}
