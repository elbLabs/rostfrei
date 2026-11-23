use super::{
    JetStreamServerLimits, ServerHealth, ServerMetrics, ServerMonitorFailure, ServerMonitoring,
    ServerSample, ServerSnapshot,
};
use futures_util::future::join_all;
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use std::{collections::BTreeSet, time::Duration};

const MAX_SOURCES: usize = 32;
const CONCURRENT_NODES: usize = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const HEALTH_BODY_LIMIT: usize = 4 * 1024;
const VARZ_BODY_LIMIT: usize = 128 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum NatsServerMonitorConfigError {
    #[error(
        "NATS monitor labels must be 1–80 ASCII letters, digits, spaces, hyphens or underscores"
    )]
    InvalidLabel,
    #[error("NATS monitors require an HTTP(S) base URL without credentials, query or fragment")]
    InvalidEndpoint,
    #[error("configure 1–32 individual NATS monitors")]
    SourceCount,
    #[error("NATS monitor labels must be unique")]
    DuplicateLabel,
    #[error("could not initialize the NATS monitor HTTP client")]
    Client,
}

/// An explicitly configured monitoring base URL for one node (not a load balancer).
/// Only a public label is shared with inspection clients.
pub struct NatsServerMonitorSource {
    label: String,
    health: Url,
    varz: Url,
}

impl NatsServerMonitorSource {
    pub fn new(
        label: impl Into<String>,
        base_url: &str,
    ) -> Result<Self, NatsServerMonitorConfigError> {
        let label = label.into();
        if label.trim().is_empty()
            || label.len() > 80
            || !label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_'))
        {
            return Err(NatsServerMonitorConfigError::InvalidLabel);
        }
        let base =
            Url::parse(base_url).map_err(|_| NatsServerMonitorConfigError::InvalidEndpoint)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(NatsServerMonitorConfigError::InvalidEndpoint);
        }
        let with_path = |path| {
            let mut url = base.clone();
            url.path_segments_mut()
                .map_err(|()| NatsServerMonitorConfigError::InvalidEndpoint)?
                .pop_if_empty()
                .push(path);
            Ok(url)
        };
        Ok(Self {
            label,
            health: with_path("healthz")?,
            varz: with_path("varz")?,
        })
    }
}

/// Samples `/healthz` and `/varz` over direct HTTP(S) without credentials.
pub struct NatsServerMonitor {
    client: Client,
    sources: Vec<NatsServerMonitorSource>,
}

impl NatsServerMonitor {
    pub fn new(
        sources: impl IntoIterator<Item = NatsServerMonitorSource>,
    ) -> Result<Self, NatsServerMonitorConfigError> {
        let sources: Vec<_> = sources
            .into_iter()
            .take(MAX_SOURCES.saturating_add(1))
            .collect();
        if sources.is_empty() || sources.len() > MAX_SOURCES {
            return Err(NatsServerMonitorConfigError::SourceCount);
        }
        let mut labels = BTreeSet::new();
        if sources
            .iter()
            .any(|source| !labels.insert(source.label.trim()))
        {
            return Err(NatsServerMonitorConfigError::DuplicateLabel);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| NatsServerMonitorConfigError::Client)?;
        Ok(Self { client, sources })
    }

    async fn sample(&self, source: &NatsServerMonitorSource) -> ServerSample {
        let (health, metrics) =
            tokio::join!(self.health(&source.health), self.metrics(&source.varz));
        ServerSample {
            label: source.label.clone(),
            health: health.into(),
            metrics: metrics.into(),
        }
    }

    async fn health(&self, url: &Url) -> Result<ServerHealth, ServerMonitorFailure> {
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(request_failure)?;
        let code = response.status();
        if !matches!(
            code,
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE | StatusCode::INTERNAL_SERVER_ERROR
        ) {
            return Err(http_failure(code));
        }
        let Healthz { status } = read_json(response, HEALTH_BODY_LIMIT).await?;
        match (code, status.as_str()) {
            (StatusCode::OK, "ok") => Ok(ServerHealth::Healthy),
            (
                StatusCode::SERVICE_UNAVAILABLE | StatusCode::INTERNAL_SERVER_ERROR,
                "error" | "unavailable",
            ) => Ok(ServerHealth::Unhealthy),
            _ => Err(ServerMonitorFailure::InvalidResponse),
        }
    }

    async fn metrics(&self, url: &Url) -> Result<ServerMetrics, ServerMonitorFailure> {
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(request_failure)?;
        if response.status() != StatusCode::OK {
            return Err(http_failure(response.status()));
        }
        let varz: Varz = read_json(response, VARZ_BODY_LIMIT).await?;
        let resident_memory_bytes = varz.mem.and_then(|v| u64::try_from(v).ok());
        let version = varz.version.as_deref().and_then(release_version);
        let uptime = varz.uptime.filter(|text| valid_uptime(text));
        let jet_stream_limits = varz.jetstream.and_then(|js| js.config).and_then(|config| {
            let max_memory_bytes = config.max_memory.and_then(|v| u64::try_from(v).ok());
            let max_storage_bytes = config.max_storage.and_then(|v| u64::try_from(v).ok());
            (max_memory_bytes.is_some() || max_storage_bytes.is_some()).then_some(
                JetStreamServerLimits {
                    max_memory_bytes,
                    max_storage_bytes,
                },
            )
        });
        if resident_memory_bytes.is_none()
            && version.is_none()
            && uptime.is_none()
            && jet_stream_limits.is_none()
        {
            return Err(ServerMonitorFailure::InvalidResponse);
        }
        Ok(ServerMetrics {
            resident_memory_bytes,
            version,
            uptime,
            jet_stream_limits,
        })
    }
}

#[async_trait::async_trait]
impl ServerMonitoring for NatsServerMonitor {
    async fn snapshot(&self) -> ServerSnapshot {
        let mut servers = Vec::with_capacity(self.sources.len());
        for batch in self.sources.chunks(CONCURRENT_NODES) {
            let mut probes = Vec::with_capacity(batch.len());
            for source in batch {
                probes.push(self.sample(source));
            }
            servers.extend(join_all(probes).await);
        }
        ServerSnapshot { servers }
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "maps a consumed reqwest error without carrying sensitive details"
)]
fn request_failure(error: reqwest::Error) -> ServerMonitorFailure {
    if error.is_timeout() {
        ServerMonitorFailure::TimedOut
    } else {
        ServerMonitorFailure::Inaccessible
    }
}

const fn http_failure(code: StatusCode) -> ServerMonitorFailure {
    match code {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
            ServerMonitorFailure::Inaccessible
        }
        _ => ServerMonitorFailure::UnexpectedHttpStatus,
    }
}

async fn read_json<T: serde::de::DeserializeOwned>(
    mut response: Response,
    limit: usize,
) -> Result<T, ServerMonitorFailure> {
    if response
        .content_length()
        .is_some_and(|length| usize::try_from(length).map_or(true, |length| length > limit))
    {
        return Err(ServerMonitorFailure::BodyTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_failure)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ServerMonitorFailure::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| ServerMonitorFailure::InvalidResponse)
}

#[derive(Deserialize)]
struct Healthz {
    status: String,
}

#[derive(Deserialize)]
struct Varz {
    mem: Option<i64>,
    version: Option<String>,
    uptime: Option<String>,
    jetstream: Option<JetStream>,
}

#[derive(Deserialize)]
struct JetStream {
    config: Option<Limits>,
}

#[derive(Deserialize)]
struct Limits {
    max_memory: Option<i64>,
    max_storage: Option<i64>,
}

// Keep the numeric release only; suffixes can contain arbitrary build metadata.
fn release_version(text: &str) -> Option<String> {
    let release = text.split(['-', '+']).next()?;
    let mut parts = release.split('.');
    for _ in 0..3 {
        let part = parts.next()?;
        if !matches!(part.len(), 1..=5) || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    parts.next().is_none().then(|| release.to_owned())
}

// NATS reports durations such as `1d2h3m4s`. Do not forward arbitrary strings.
fn valid_uptime(mut text: &str) -> bool {
    if text.len() > 40 {
        return false;
    }
    for unit in ['y', 'd', 'h', 'm', 's'] {
        if let Some((count, rest)) = text.split_once(unit) {
            if count.is_empty() || !count.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
            if unit == 's' {
                return rest.is_empty();
            }
            text = rest;
        }
    }
    false
}
