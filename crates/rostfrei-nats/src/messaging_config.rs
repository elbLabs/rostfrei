use std::{fmt, future::Future, path::PathBuf, pin::Pin, sync::Arc, time::Duration};

use async_nats::{ConnectOptions, ServerAddr};
use percent_encoding::percent_decode_str;
use rostfrei_messaging_core::{AddressKind, ApplicationName, TrafficScope};

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
        let final_index = tokens
            .len()
            .checked_sub(1)
            .ok_or(NatsError::Configuration)?;
        if tokens.iter().any(|token| token.is_empty())
            || tokens.iter().enumerate().any(|(index, token)| {
                (*token == ">" && index != final_index)
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
    application: ApplicationName,
    traffic_scope: TrafficScope,
    command: StreamName,
    command_response: StreamName,
    integration_event: StreamName,
    quarantine: StreamName,
}

impl MessagingTopology {
    pub fn new(
        application: ApplicationName,
        command_stream: StreamName,
        integration_event_stream: StreamName,
        quarantine_stream: StreamName,
    ) -> Result<Self, NatsError> {
        Self::new_in_scope(
            application,
            TrafficScope::Normal,
            command_stream,
            integration_event_stream,
            quarantine_stream,
        )
    }

    pub fn new_in_scope(
        application: ApplicationName,
        traffic_scope: TrafficScope,
        command_stream: StreamName,
        integration_event_stream: StreamName,
        quarantine_stream: StreamName,
    ) -> Result<Self, NatsError> {
        let command_response_stream = StreamName::new(format!(
            "{}_COMMAND_RESPONSES",
            traffic_stream_prefix(application.as_str(), traffic_scope)
        ))?;
        Self::new_with_command_response_stream_in_scope(
            application,
            traffic_scope,
            command_stream,
            command_response_stream,
            integration_event_stream,
            quarantine_stream,
        )
    }

    pub fn new_with_command_response_stream(
        application: ApplicationName,
        command_stream: StreamName,
        command_response_stream: StreamName,
        integration_event_stream: StreamName,
        quarantine_stream: StreamName,
    ) -> Result<Self, NatsError> {
        Self::new_with_command_response_stream_in_scope(
            application,
            TrafficScope::Normal,
            command_stream,
            command_response_stream,
            integration_event_stream,
            quarantine_stream,
        )
    }

    pub fn new_with_command_response_stream_in_scope(
        application: ApplicationName,
        traffic_scope: TrafficScope,
        command_stream: StreamName,
        command_response_stream: StreamName,
        integration_event_stream: StreamName,
        quarantine_stream: StreamName,
    ) -> Result<Self, NatsError> {
        if command_stream == command_response_stream
            || command_stream == integration_event_stream
            || command_stream == quarantine_stream
            || command_response_stream == integration_event_stream
            || command_response_stream == quarantine_stream
            || integration_event_stream == quarantine_stream
        {
            return Err(NatsError::Configuration);
        }
        Ok(Self {
            application,
            traffic_scope,
            command: command_stream,
            command_response: command_response_stream,
            integration_event: integration_event_stream,
            quarantine: quarantine_stream,
        })
    }

    pub fn for_application(application: &ApplicationName) -> Result<Self, NatsError> {
        Self::for_application_in_scope(application, TrafficScope::Normal)
    }

    pub fn for_application_in_scope(
        application: &ApplicationName,
        traffic_scope: TrafficScope,
    ) -> Result<Self, NatsError> {
        let prefix = traffic_stream_prefix(application.as_str(), traffic_scope);
        Self::new_with_command_response_stream_in_scope(
            application.clone(),
            traffic_scope,
            StreamName::new(format!("{prefix}_COMMANDS"))?,
            StreamName::new(format!("{prefix}_COMMAND_RESPONSES"))?,
            StreamName::new(format!("{prefix}_INTEGRATION_EVENTS"))?,
            StreamName::new(format!("{prefix}_QUARANTINE"))?,
        )
    }

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub const fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope
    }

    pub const fn command_stream(&self) -> &StreamName {
        &self.command
    }

    pub const fn command_response_stream(&self) -> &StreamName {
        &self.command_response
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
            AddressKind::CommandResponse => Some(&self.command_response),
            AddressKind::IntegrationEvent => Some(&self.integration_event),
            AddressKind::Query => None,
        }
    }
}

pub fn traffic_subject_prefix(application: &str, traffic_scope: TrafficScope) -> String {
    traffic_scope.subject_segment().map_or_else(
        || application.to_owned(),
        |scope| format!("{application}.{scope}"),
    )
}

pub fn traffic_stream_prefix(application: &str, traffic_scope: TrafficScope) -> String {
    let application = application
        .bytes()
        .map(|byte| match byte {
            b'-' => b'_',
            _ => byte.to_ascii_uppercase(),
        })
        .map(char::from)
        .collect::<String>();
    match traffic_scope {
        TrafficScope::Normal => application,
        TrafficScope::Test => format!("{application}__TEST"),
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

    const fn validate(self) -> Result<(), NatsError> {
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

/// Connection settings with application-owned authentication and TLS configuration.
///
/// Rostfrei owns initial connection establishment, reconnect policy, lifecycle
/// events, version checks, health tracking, and graceful drain. Authentication
/// builders replace the previously selected authentication method.
#[derive(Clone)]
pub struct NatsConnectionConfig {
    client_name: String,
    server_urls: Vec<String>,
    connection_timeout: Duration,
    drain_timeout: Duration,
    minimum_server_version: ServerVersion,
    authentication: Option<Authentication>,
    tls_required: bool,
    root_certificates: Option<PathBuf>,
    client_certificate: Option<(PathBuf, PathBuf)>,
}

#[derive(Clone)]
enum Authentication {
    Token(String),
    UserAndPassword(NatsUserPassword),
    Callback(Arc<AuthenticationCallback>),
}

type AuthenticationCallback = dyn Fn(
        Vec<u8>,
    ) -> Pin<
        Box<dyn Future<Output = Result<async_nats::Auth, async_nats::AuthError>> + Send + Sync>,
    > + Send
    + Sync;

impl NatsConnectionConfig {
    /// Creates a connection configuration from comma-separated server URLs.
    ///
    /// URL credentials are percent-decoded and applied to the whole connection,
    /// including reconnects and discovered servers. Without explicit credentials,
    /// every URL must either omit credentials or contain the same complete pair.
    pub fn new(client_name: impl Into<String>, server_urls: impl Into<String>) -> Self {
        Self::from_server_pool(client_name, server_urls.into().split(','))
    }

    /// Creates a server pool using the same credential rules as [`Self::new`].
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
            authentication: None,
            tls_required: false,
            root_certificates: None,
            client_certificate: None,
        }
    }

    /// Sets a username and password for every server, including discovered servers.
    ///
    /// These values are used literally, without percent-decoding. Both must be
    /// nonempty. URLs may omit credentials, but any embedded credentials must be
    /// complete and match this pair after decoding. Invalid or conflicting
    /// credentials are rejected by [`Self::validate`] and [`crate::connect`].
    /// This replaces any previously selected authentication method.
    #[must_use]
    pub fn with_user_and_password(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.authentication = Some(Authentication::UserAndPassword(NatsUserPassword {
            username: username.into(),
            password: password.into(),
        }));
        self
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

    /// Authenticates using a token, replacing any previous authentication method.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.authentication = Some(Authentication::Token(token.into()));
        self
    }

    /// Obtains authentication for each connection attempt, including reconnects.
    ///
    /// The callback receives the server nonce and returns credentials or a signed
    /// challenge response. It replaces any previous authentication method and is
    /// bounded by the connection timeout. It does not receive lifecycle events.
    #[must_use]
    pub fn with_auth_callback<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<async_nats::Auth, async_nats::AuthError>>
            + Send
            + Sync
            + 'static,
    {
        self.authentication = Some(Authentication::Callback(Arc::new(move |nonce| {
            Box::pin(callback(nonce))
        })));
        self
    }

    /// Requires TLS using the system trust roots unless custom roots are supplied.
    #[must_use]
    pub const fn with_tls(mut self) -> Self {
        self.tls_required = true;
        self
    }

    /// Requires TLS and trusts the certificates in a PEM bundle.
    #[must_use]
    pub fn with_root_certificates(mut self, path: impl Into<PathBuf>) -> Self {
        self.root_certificates = Some(path.into());
        self.tls_required = true;
        self
    }

    /// Requires TLS and presents a PEM client certificate chain and private key.
    #[must_use]
    pub fn with_client_certificate(
        mut self,
        certificate: impl Into<PathBuf>,
        key: impl Into<PathBuf>,
    ) -> Self {
        self.client_certificate = Some((certificate.into(), key.into()));
        self.tls_required = true;
        self
    }

    pub(crate) fn connect_options(&self) -> async_nats::ConnectOptions {
        let mut options = match &self.authentication {
            None => async_nats::ConnectOptions::new(),
            Some(Authentication::Token(token)) => {
                async_nats::ConnectOptions::with_token(token.clone())
            }
            Some(Authentication::UserAndPassword(authentication)) => authentication
                .clone()
                .apply_to(async_nats::ConnectOptions::new()),
            Some(Authentication::Callback(callback)) => {
                let callback = Arc::clone(callback);
                let connection_timeout = self.connection_timeout;
                async_nats::ConnectOptions::with_auth_callback(move |nonce| {
                    let authentication = callback(nonce);
                    async move {
                        tokio::time::timeout(connection_timeout, authentication)
                            .await
                            .map_err(|_| async_nats::AuthError::new("authentication timed out"))?
                    }
                })
            }
        };
        if let Some(path) = &self.root_certificates {
            options = options.add_root_certificates(path.clone());
        }
        if let Some((certificate, key)) = &self.client_certificate {
            options = options.add_client_certificate(certificate.clone(), key.clone());
        }
        options.require_tls(self.tls_required)
    }

    pub fn validate(&self) -> Result<(), NatsError> {
        self.connection_settings().map(|_| ())
    }

    pub(crate) fn connection_settings(
        &self,
    ) -> Result<(Vec<ServerAddr>, Option<NatsUserPassword>), NatsError> {
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
        let mut authentication = match &self.authentication {
            Some(Authentication::UserAndPassword(authentication)) => {
                authentication.validate()?;
                Some(authentication.clone())
            }
            _ => None,
        };
        let mut has_anonymous_url = false;
        let mut servers = Vec::with_capacity(self.server_urls.len());
        for server_url in &self.server_urls {
            let (server, url_authentication) = parse_server_url(server_url)?;
            if let Some(url_authentication) = url_authentication {
                if matches!(
                    self.authentication,
                    Some(Authentication::Token(_) | Authentication::Callback(_))
                ) {
                    return Err(NatsError::Configuration);
                }
                if authentication
                    .as_ref()
                    .is_some_and(|current| *current != url_authentication)
                {
                    return Err(NatsError::Configuration);
                }
                authentication = Some(url_authentication);
            } else {
                has_anonymous_url = true;
            }
            servers.push(server);
        }
        if self.authentication.is_none() && authentication.is_some() && has_anonymous_url {
            return Err(NatsError::Configuration);
        }
        Ok((servers, authentication))
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub const fn server_count(&self) -> usize {
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
}

#[derive(Clone, Eq, PartialEq)]
pub struct NatsUserPassword {
    username: String,
    password: String,
}

impl NatsUserPassword {
    const fn validate(&self) -> Result<(), NatsError> {
        if self.username.is_empty() || self.password.is_empty() {
            return Err(NatsError::Configuration);
        }
        Ok(())
    }

    pub(crate) fn apply_to(self, options: ConnectOptions) -> ConnectOptions {
        options.user_and_password(self.username, self.password)
    }
}

impl fmt::Debug for NatsUserPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NatsUserPassword([REDACTED])")
    }
}

fn parse_server_url(server_url: &str) -> Result<(ServerAddr, Option<NatsUserPassword>), NatsError> {
    if server_url.is_empty()
        || server_url.trim() != server_url
        || server_url.chars().any(char::is_control)
    {
        return Err(NatsError::Configuration);
    }
    // The URL parser normalizes empty userinfo away, so inspect the authority
    // too: `nats://:@host` and `nats://@host` are incomplete credentials.
    let authority = server_url
        .split_once("://")
        .map_or(server_url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .ok_or(NatsError::Configuration)?;
    let mut url = server_url
        .parse::<ServerAddr>()
        .map_err(|_| NatsError::Configuration)?
        .into_inner();
    if url.host_str().is_none_or(str::is_empty) {
        return Err(NatsError::Configuration);
    }
    let authentication = if authority.contains('@') {
        let credentials = NatsUserPassword {
            username: decode_credential(url.username())?,
            password: decode_credential(url.password().ok_or(NatsError::Configuration)?)?,
        };
        credentials.validate()?;
        Some(credentials)
    } else {
        None
    };
    // Never give credential-bearing addresses to async-nats: they can appear
    // in upstream diagnostics, and URL credentials are not connection-wide auth.
    url.set_password(None)
        .map_err(|()| NatsError::Configuration)?;
    url.set_username("")
        .map_err(|()| NatsError::Configuration)?;
    let server = ServerAddr::from_url(url).map_err(|_| NatsError::Configuration)?;
    Ok((server, authentication))
}

fn decode_credential(value: &str) -> Result<String, NatsError> {
    if value.split('%').skip(1).any(|suffix| {
        suffix
            .as_bytes()
            .get(..2)
            .is_none_or(|escape| !escape.iter().all(u8::is_ascii_hexdigit))
    }) {
        return Err(NatsError::Configuration);
    }
    percent_decode_str(value)
        .decode_utf8()
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| NatsError::Configuration)
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
            .field("has_authentication", &self.authentication.is_some())
            .field("tls_required", &self.tls_required)
            .field("has_root_certificates", &self.root_certificates.is_some())
            .field("has_client_certificate", &self.client_certificate.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SERVER_VERSION: Result<(), NatsError> = ServerVersion::new(2, 10, 0).validate();
    const INVALID_SERVER_VERSION: Result<(), NatsError> = ServerVersion::new(0, 10, 0).validate();

    #[test]
    fn anonymous_connections_remain_supported() {
        let config = NatsConnectionConfig::new("anonymous", "localhost:4222,nats://localhost:4223");
        let (servers, authentication) = config.connection_settings().unwrap();
        assert_eq!(servers.len(), 2);
        assert!(authentication.is_none());
    }

    #[test]
    fn explicit_credentials_are_literal_and_connection_wide() {
        let config = NatsConnectionConfig::from_server_pool(
            "explicit",
            ["localhost:4222", "tls://localhost:4223"],
        )
        .with_user_and_password("user+name", " literal%40password ");
        let (servers, authentication) = config.connection_settings().unwrap();
        let authentication = authentication.unwrap();
        assert_eq!(authentication.username, "user+name");
        assert_eq!(authentication.password, " literal%40password ");
        assert_eq!(servers.len(), 2);
        assert!(servers.iter().all(|server| !server.has_user_pass()));
    }

    #[test]
    fn url_credentials_are_decoded_once_and_removed_from_addresses() {
        let config = NatsConnectionConfig::new(
            "encoded",
            "nats://%C3%BCser+name:p%40ss%3A%2F%25%23%3F%2C%20%252F@[::1]:4222",
        );
        let (servers, authentication) = config.connection_settings().unwrap();
        let authentication = authentication.unwrap();
        assert_eq!(authentication.username, "üser+name");
        assert_eq!(authentication.password, "p@ss:/%#?, %2F");
        let server = servers.first().unwrap();
        assert_eq!(server.as_url_str(), "nats://[::1]:4222");
        assert!(server.username().is_none());
        assert!(server.password().is_none());
        for diagnostic in [
            format!("{config:?}"),
            format!("{servers:?}"),
            format!("{authentication:?}"),
        ] {
            for secret in ["üser", "%C3%BCser", "p@ss", "p%40ss"] {
                assert!(!diagnostic.contains(secret));
            }
        }
    }

    #[test]
    fn server_pools_compare_decoded_credentials() {
        let config = NatsConnectionConfig::new(
            "pool",
            "nats://user:p%40ss@localhost:4222,tls://%75ser:p%40%73s@localhost:4223",
        );
        let (servers, authentication) = config.connection_settings().unwrap();
        assert_eq!(authentication.unwrap().password, "p@ss");
        assert_eq!(servers[0].as_url_str(), "nats://localhost:4222");
        assert_eq!(servers[1].as_url_str(), "tls://localhost:4223");

        let config = NatsConnectionConfig::from_server_pool(
            "explicit-pool",
            ["localhost:4222", "user:p%40ss@localhost:4223"],
        )
        .with_user_and_password("user", "p@ss");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn incomplete_and_malformed_credentials_are_rejected() {
        for url in [
            "nats://user@localhost:4222",
            "nats://user:@localhost:4222",
            "nats://:password@localhost:4222",
            "nats://@localhost:4222",
            "nats://:@localhost:4222",
            "nats://user:pass%FF@localhost:4222",
            "nats://user%FF:password@localhost:4222",
            "nats://user:pass%@localhost:4222",
            "nats://user:pass%2@localhost:4222",
            "nats://user:pass%GG@localhost:4222",
            "nats://user:pass\nword@localhost:4222",
            "nats://user:password@",
        ] {
            let config = NatsConnectionConfig::new("invalid", url);
            assert_eq!(config.validate(), Err(NatsError::Configuration));
            // Explicit credentials must not conceal a broken URL credential pair.
            assert_eq!(
                config.with_user_and_password("user", "password").validate(),
                Err(NatsError::Configuration)
            );
        }
        for (username, password) in [("", "password"), ("user", ""), ("", "")] {
            assert_eq!(
                NatsConnectionConfig::new("invalid", "localhost:4222")
                    .with_user_and_password(username, password)
                    .validate(),
                Err(NatsError::Configuration)
            );
        }
    }

    #[test]
    fn conflicting_or_partially_authenticated_pools_are_rejected_in_any_order() {
        for other in [
            "nats://user:other@localhost:4223",
            "nats://other:password@localhost:4223",
            "nats://localhost:4223",
        ] {
            let authenticated = "nats://user:password@localhost:4222";
            for pool in [[authenticated, other], [other, authenticated]] {
                assert_eq!(
                    NatsConnectionConfig::from_server_pool("invalid-pool", pool).validate(),
                    Err(NatsError::Configuration)
                );
            }
        }
        for (username, password) in [("other", "password"), ("user", "other")] {
            assert_eq!(
                NatsConnectionConfig::new("conflict", "nats://user:password@localhost:4222")
                    .with_user_and_password(username, password)
                    .validate(),
                Err(NatsError::Configuration)
            );
        }
    }

    #[test]
    fn configuration_debug_and_errors_never_include_credentials() {
        for url in [
            "nats://private-user:private-password@localhost:4222",
            "bad-scheme://private-user:private-password@localhost:4222",
            "nats://private-user:private-password@",
        ] {
            let config = NatsConnectionConfig::new("redacted", url)
                .with_user_and_password("explicit-user", "explicit-password");
            let error = config.validate().unwrap_err();
            assert!(std::error::Error::source(&error).is_none());
            for diagnostic in [
                format!("{config:?}"),
                format!("{error:?}"),
                error.to_string(),
            ] {
                for secret in [
                    "private-user",
                    "private-password",
                    "explicit-user",
                    "explicit-password",
                ] {
                    assert!(!diagnostic.contains(secret));
                }
            }
        }
    }

    #[test]
    fn connection_debug_omits_authentication_secrets() {
        let config =
            NatsConnectionConfig::new("application", "nats://url-user:url-password@localhost:4222")
                .with_user_and_password("configured-user", "configured-password");
        let debug = format!("{config:?}");
        for secret in [
            "url-user",
            "url-password",
            "configured-user",
            "configured-password",
        ] {
            assert!(!debug.contains(secret));
        }
        let config = config.with_token("configured-token");
        assert!(!format!("{config:?}").contains("configured-token"));
    }

    #[test]
    fn token_and_callback_authentication_cannot_mix_with_url_credentials() {
        let config = NatsConnectionConfig::new("mixed", "nats://user:password@localhost:4222");
        assert_eq!(
            config.clone().with_token("token").validate(),
            Err(NatsError::Configuration)
        );
        assert_eq!(
            config
                .with_auth_callback(|_| async { Ok(async_nats::Auth::new()) })
                .validate(),
            Err(NatsError::Configuration)
        );
        let replaced = NatsConnectionConfig::new("replacement", "localhost:4222")
            .with_user_and_password("", "")
            .with_token("token");
        assert!(replaced.validate().is_ok());
    }

    #[test]
    fn server_versions_are_const_validated() {
        assert!(VALID_SERVER_VERSION.is_ok());
        assert!(INVALID_SERVER_VERSION.is_err());
    }

    #[test]
    fn custom_topology_rejects_aliased_stream_roles() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let shared = StreamName::new("FAST_INBOX_SHARED").unwrap();

        assert!(
            MessagingTopology::new(
                application,
                shared.clone(),
                shared,
                StreamName::new("FAST_INBOX_QUARANTINE").unwrap(),
            )
            .is_err()
        );

        assert!(
            MessagingTopology::new_with_command_response_stream(
                ApplicationName::new("fast-inbox").unwrap(),
                StreamName::new("FAST_INBOX_COMMANDS").unwrap(),
                StreamName::new("FAST_INBOX_COMMANDS").unwrap(),
                StreamName::new("FAST_INBOX_INTEGRATION_EVENTS").unwrap(),
                StreamName::new("FAST_INBOX_QUARANTINE").unwrap(),
            )
            .is_err()
        );
    }

    #[test]
    fn test_topology_derives_disjoint_stream_names() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let normal = MessagingTopology::for_application(&application).unwrap();
        let test =
            MessagingTopology::for_application_in_scope(&application, TrafficScope::Test).unwrap();

        assert_eq!(normal.command_stream().as_str(), "FAST_INBOX_COMMANDS");
        assert_eq!(test.command_stream().as_str(), "FAST_INBOX__TEST_COMMANDS");
        assert_eq!(test.traffic_scope(), TrafficScope::Test);
        assert_ne!(normal.command_stream(), test.command_stream());
    }
}
