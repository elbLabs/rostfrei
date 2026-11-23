//! Optional server-level evidence, separate from broker account quotas.

use serde::{Deserialize, Serialize};

#[cfg(feature = "nats-host")]
mod nats;
#[cfg(feature = "nats-host")]
pub use nats::{NatsServerMonitor, NatsServerMonitorConfigError, NatsServerMonitorSource};

/// A read-only source of samples for operator-configured server nodes.
#[async_trait::async_trait]
pub trait ServerMonitoring: Send + Sync {
    async fn snapshot(&self) -> ServerSnapshot;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSnapshot {
    pub servers: Vec<ServerSample>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSample {
    /// A public display label, never a monitoring endpoint or broker address.
    pub label: String,
    pub health: ServerProbe<ServerHealth>,
    pub metrics: ServerProbe<ServerMetrics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ServerProbe<T> {
    Available { value: T },
    Unavailable { reason: ServerMonitorFailure },
}

impl<T> From<Result<T, ServerMonitorFailure>> for ServerProbe<T> {
    fn from(result: Result<T, ServerMonitorFailure>) -> Self {
        match result {
            Ok(value) => Self::Available { value },
            Err(reason) => Self::Unavailable { reason },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerMonitorFailure {
    Inaccessible,
    TimedOut,
    UnexpectedHttpStatus,
    BodyTooLarge,
    InvalidResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerHealth {
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMetrics {
    /// Process RSS, not `JetStream` memory usage or a configured limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_memory_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jet_stream_limits: Option<JetStreamServerLimits>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JetStreamServerLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_memory_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_storage_bytes: Option<u64>,
}
