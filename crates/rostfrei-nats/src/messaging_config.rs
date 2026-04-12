use std::{fmt, time::Duration};

use rostfrei_messaging_core::AddressKind;

use crate::error::NatsError;

pub const MAX_STREAM_NAME_BYTES: usize = 255;
pub const MAX_SUBJECT_FILTER_BYTES: usize = 512;
pub const MAX_QUEUE_GROUP_BYTES: usize = 255;
pub const MAX_CLIENT_NAME_BYTES: usize = 255;
pub const MINIMUM_NATS_SERVER_VERSION: ServerVersion = ServerVersion::new(2, 10, 0);
pub const DEFAULT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StreamName(String);

impl StreamName {
    pub fn new(value: impl Into<String>) -> Result<Self, NatsError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_STREAM_NAME_BYTES
            || !value.is_ascii()
            || value.bytes().any(|byte| {
                byte.is_ascii_whitespace()
                    || byte.is_ascii_control()
                    || matches!(byte, b'.' | b'*' | b'>')
            })
        {
            return Err(NatsError::Configuration);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for StreamName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for StreamName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SubjectFilter(String);

impl SubjectFilter {
    pub fn new(value: impl Into<String>) -> Result<Self, NatsError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_SUBJECT_FILTER_BYTES
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        {
            return Err(NatsError::Configuration);
        }

        let tokens = value.split('.').collect::<Vec<_>>();
        if tokens.iter().any(|token| token.is_empty())
            || tokens.iter().enumerate().any(|(index, token)| {
                (*token == ">" && index + 1 != tokens.len())
                    || (token.contains('>') && *token != ">")
                    || (token.contains('*') && *token != "*")
            })
        {
            return Err(NatsError::Configuration);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn has_wildcards(&self) -> bool {
        self.0.contains(['*', '>'])
    }
}

impl AsRef<str> for SubjectFilter {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SubjectFilter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueueGroup(String);

impl QueueGroup {
    pub fn new(value: impl Into<String>) -> Result<Self, NatsError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_QUEUE_GROUP_BYTES
            || !value.is_ascii()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(NatsError::Configuration);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for QueueGroup {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for QueueGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessagingTopology {
    command: StreamName,
    integration_event: StreamName,
    quarantine: StreamName,
}

impl MessagingTopology {
    pub const fn new(
        command_stream: StreamName,
        integration_event_stream: StreamName,
        quarantine_stream: StreamName,
    ) -> Self {
        Self {
            command: command_stream,
            integration_event: integration_event_stream,
            quarantine: quarantine_stream,
        }
    }

    pub const fn command_stream(&self) -> &StreamName {
        &self.command
    }

    pub const fn integration_event_stream(&self) -> &StreamName {
        &self.integration_event
    }

    pub const fn quarantine_stream(&self) -> &StreamName {
        &self.quarantine
    }

    pub const fn stream_for(&self, kind: AddressKind) -> Option<&StreamName> {
        match kind {
            AddressKind::Command => Some(&self.command),
            AddressKind::IntegrationEvent => Some(&self.integration_event),
            AddressKind::Query => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ServerVersion {
    major: i64,
    minor: i64,
    patch: i64,
}

impl ServerVersion {
    pub const fn new(major: i64, minor: i64, patch: i64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub const fn major(self) -> i64 {
        self.major
    }

    pub const fn minor(self) -> i64 {
        self.minor
    }

    pub const fn patch(self) -> i64 {
        self.patch
    }

    fn validate(self) -> Result<(), NatsError> {
        if self.major < 1 || self.minor < 0 || self.patch < 0 {
            return Err(NatsError::Configuration);
        }
        Ok(())
    }
}

impl fmt::Display for ServerVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone)]
pub struct NatsConnectionConfig {
    client_name: String,
    server_urls: Vec<String>,
    connection_timeout: Duration,
    drain_timeout: Duration,
    minimum_server_version: ServerVersion,
}

impl NatsConnectionConfig {
    pub fn new(client_name: impl Into<String>, server_urls: impl Into<String>) -> Self {
        Self {
            client_name: client_name.into(),
            server_urls: server_urls.into().split(',').map(str::to_owned).collect(),
            connection_timeout: DEFAULT_CONNECTION_TIMEOUT,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            minimum_server_version: MINIMUM_NATS_SERVER_VERSION,
        }
    }

    pub fn from_server_pool<I, S>(client_name: impl Into<String>, server_urls: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            client_name: client_name.into(),
            server_urls: server_urls.into_iter().map(Into::into).collect(),
            connection_timeout: DEFAULT_CONNECTION_TIMEOUT,
            drain_timeout: DEFAULT_DRAIN_TIMEOUT,
            minimum_server_version: MINIMUM_NATS_SERVER_VERSION,
        }
    }

    #[must_use]
    pub const fn with_connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_timeout = timeout;
        self
    }

    #[must_use]
    pub const fn with_minimum_server_version(mut self, version: ServerVersion) -> Self {
        self.minimum_server_version = version;
        self
    }

    pub fn validate(&self) -> Result<(), NatsError> {
        if self.client_name.is_empty()
            || self.client_name.len() > MAX_CLIENT_NAME_BYTES
            || self.client_name.trim() != self.client_name
            || !self.client_name.is_ascii()
            || self.client_name.chars().any(char::is_control)
            || self.server_urls.is_empty()
            || self.connection_timeout.is_zero()
            || self.drain_timeout.is_zero()
        {
            return Err(NatsError::Configuration);
        }
        self.minimum_server_version.validate()?;
        for server_url in &self.server_urls {
            if server_url.is_empty() || server_url.trim() != server_url {
                return Err(NatsError::Configuration);
            }
            server_url
                .parse::<async_nats::ServerAddr>()
                .map_err(|_| NatsError::Configuration)?;
        }
        Ok(())
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn server_count(&self) -> usize {
        self.server_urls.len()
    }

    pub const fn connection_timeout(&self) -> Duration {
        self.connection_timeout
    }

    pub const fn drain_timeout(&self) -> Duration {
        self.drain_timeout
    }

    pub const fn minimum_server_version(&self) -> ServerVersion {
        self.minimum_server_version
    }

    pub(crate) fn server_addrs(&self) -> Result<Vec<async_nats::ServerAddr>, NatsError> {
        self.validate()?;
        self.server_urls
            .iter()
            .map(|server_url| server_url.parse())
            .collect::<Result<_, _>>()
            .map_err(|_| NatsError::Configuration)
    }
}

impl fmt::Debug for NatsConnectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NatsConnectionConfig")
            .field("client_name", &self.client_name)
            .field("server_count", &self.server_urls.len())
            .field("connection_timeout", &self.connection_timeout)
            .field("drain_timeout", &self.drain_timeout)
            .field("minimum_server_version", &self.minimum_server_version)
            .finish()
    }
}
