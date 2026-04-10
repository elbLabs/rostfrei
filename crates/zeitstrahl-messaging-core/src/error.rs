use std::fmt;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ContractErrorKind {
    Empty,
    TooLong,
    InvalidFormat,
    WrongAddressKind,
    Wildcard,
    ControlCharacter,
    Reserved,
    TooManyEntries,
    OutOfRange,
}

impl fmt::Display for ContractErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self {
            Self::Empty => "must not be empty",
            Self::TooLong => "is too long",
            Self::InvalidFormat => "has an invalid format",
            Self::WrongAddressKind => "has the wrong address kind",
            Self::Wildcard => "contains a wildcard",
            Self::ControlCharacter => "contains a control character",
            Self::Reserved => "is reserved for the transport adapter",
            Self::TooManyEntries => "contains too many entries",
            Self::OutOfRange => "is outside the supported range",
        };
        formatter.write_str(description)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{field} {kind}")]
pub struct ContractError {
    kind: ContractErrorKind,
    field: &'static str,
    actual: Option<usize>,
    maximum: Option<usize>,
}

impl ContractError {
    pub(crate) const fn new(kind: ContractErrorKind, field: &'static str) -> Self {
        Self {
            kind,
            field,
            actual: None,
            maximum: None,
        }
    }

    pub(crate) const fn bounded(
        kind: ContractErrorKind,
        field: &'static str,
        actual: usize,
        maximum: usize,
    ) -> Self {
        Self {
            kind,
            field,
            actual: Some(actual),
            maximum: Some(maximum),
        }
    }

    pub const fn kind(&self) -> ContractErrorKind {
        self.kind
    }

    pub const fn field(&self) -> &'static str {
        self.field
    }

    pub const fn actual(&self) -> Option<usize> {
        self.actual
    }

    pub const fn maximum(&self) -> Option<usize> {
        self.maximum
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MessageBuildErrorKind {
    Serialization,
    PayloadTooLarge,
    InvalidMaximum,
}

impl fmt::Display for MessageBuildErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self {
            Self::Serialization => "message serialization failed",
            Self::PayloadTooLarge => "message payload is too large",
            Self::InvalidMaximum => "message payload maximum is invalid",
        };
        formatter.write_str(description)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct MessageBuildError {
    kind: MessageBuildErrorKind,
    actual: Option<usize>,
    maximum: Option<usize>,
}

impl MessageBuildError {
    pub(crate) const fn serialization() -> Self {
        Self {
            kind: MessageBuildErrorKind::Serialization,
            actual: None,
            maximum: None,
        }
    }

    pub(crate) const fn payload_too_large(actual: usize, maximum: usize) -> Self {
        Self {
            kind: MessageBuildErrorKind::PayloadTooLarge,
            actual: Some(actual),
            maximum: Some(maximum),
        }
    }

    pub(crate) const fn invalid_maximum(maximum: usize) -> Self {
        Self {
            kind: MessageBuildErrorKind::InvalidMaximum,
            actual: None,
            maximum: Some(maximum),
        }
    }

    pub const fn kind(&self) -> MessageBuildErrorKind {
        self.kind
    }

    pub const fn actual(&self) -> Option<usize> {
        self.actual
    }

    pub const fn maximum(&self) -> Option<usize> {
        self.maximum
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PublishErrorKind {
    #[error("publisher is unavailable")]
    Unavailable,
    #[error("publication was rejected")]
    Rejected,
    #[error("publication confirmation timed out")]
    Timeout,
    #[error("publisher configuration is invalid")]
    InvalidConfiguration,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct PublishError {
    kind: PublishErrorKind,
}

impl PublishError {
    pub const fn new(kind: PublishErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> PublishErrorKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum ConsumeErrorKind {
    #[error("consumer configuration is invalid")]
    InvalidConfiguration,
    #[error("consumer is unavailable")]
    Unavailable,
    #[error("delivery disposition could not be applied")]
    Disposition,
    #[error("consumer ended unexpectedly")]
    Ended,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct ConsumeError {
    kind: ConsumeErrorKind,
}

impl ConsumeError {
    pub const fn new(kind: ConsumeErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> ConsumeErrorKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum QueryRequestErrorKind {
    #[error("query request serialization failed")]
    Serialization,
    #[error("query response is too large")]
    ResponseTooLarge,
    #[error("query request timed out")]
    Timeout,
    #[error("query requester is unavailable")]
    Unavailable,
    #[error("query request was rejected")]
    Rejected,
    #[error("query response is invalid")]
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct QueryRequestError {
    kind: QueryRequestErrorKind,
}

impl QueryRequestError {
    pub const fn new(kind: QueryRequestErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> QueryRequestErrorKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum QueryServerErrorKind {
    #[error("query server is unavailable")]
    Unavailable,
    #[error("query server configuration is invalid")]
    InvalidConfiguration,
    #[error("query request could not be decoded")]
    InvalidRequest,
    #[error("query response could not be encoded")]
    ResponseSerialization,
    #[error("query server ended unexpectedly")]
    Ended,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct QueryServerError {
    kind: QueryServerErrorKind,
}

impl QueryServerError {
    pub const fn new(kind: QueryServerErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> QueryServerErrorKind {
        self.kind
    }
}
