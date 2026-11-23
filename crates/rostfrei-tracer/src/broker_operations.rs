//! Read-only, account-scoped broker evidence. This is a sampling API, not an
//! atomic inventory, continuous monitor, or proof of application worker health.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "nats-host")]
mod nats;
#[cfg(feature = "nats-host")]
pub use nats::{NatsBrokerOperations, NatsBrokerSource, NatsBrokerSourceError};

#[async_trait]
pub trait BrokerOperations: Send + Sync {
    /// Sample configured sources without changing them. Keep failed sources in
    /// the result and report unobserved data explicitly, not as zero usage.
    async fn snapshot(&self) -> BrokerOperationsSnapshot;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerOperationsSnapshot {
    pub sources: Vec<BrokerSourceSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerSourceSnapshot {
    pub label: String,
    pub connection: BrokerConnectionStatus,
    pub account: BrokerAccount,
    pub streams: BrokerInventory<BrokerStream>,
    pub kv_buckets: BrokerInventory<BrokerKvBucket>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Closed,
    TimedOut,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerReadStatus {
    /// The read finished and did not report omitted rows. Not an atomic snapshot.
    Complete,
    /// A local result bound omitted rows.
    Truncated,
    /// A request or the source deadline expired.
    TimedOut,
    /// Complete evidence could not be obtained (e.g. a rejected request or
    /// broker-reported missing rows).
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerInventory<T> {
    /// Non-complete sections may retain evidence obtained before interruption.
    pub status: BrokerReadStatus,
    pub items: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerAccount {
    pub status: BrokerReadStatus,
    pub usage: Option<BrokerAccountUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerAccountUsage {
    pub memory: BrokerCapacity,
    pub storage: BrokerCapacity,
    pub stream_count: usize,
    pub consumer_count: usize,
    /// Replica-class quotas are distinct, not to be summed into a global limit.
    pub tiers: Vec<BrokerAccountTier>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerAccountTier {
    pub name: String,
    pub memory: BrokerCapacity,
    pub storage: BrokerCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerCapacity {
    pub used_bytes: u64,
    /// `null` means no finite limit was reported at this level; not zero.
    pub limit_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerStorage {
    Memory,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerStream {
    pub name: String,
    pub storage: BrokerStorage,
    pub replicas: usize,
    pub messages: u64,
    pub capacity: BrokerCapacity,
    pub consumer_count: usize,
    pub consumers: BrokerInventory<BrokerConsumer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerConsumerKind {
    Pull,
    Push,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrokerConsumerLiveness {
    /// A push subscription was bound when the broker sampled the consumer.
    PushBound,
    PushUnbound,
    /// At least one pull was waiting at the broker. Not worker/business health.
    PullWaiting,
    /// No observable pull wait does not distinguish idle and absent workers.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerConsumer {
    pub name: String,
    pub kind: BrokerConsumerKind,
    pub durable: bool,
    pub paused: bool,
    pub liveness: BrokerConsumerLiveness,
    /// Matching messages pending delivery, not a stream-sequence difference.
    pub pending: u64,
    pub ack_pending: usize,
    pub redelivered: usize,
    pub waiting: usize,
    /// Decimal strings preserve the exact `JetStream` positions for browser clients.
    pub delivered_stream_sequence: String,
    pub ack_floor_stream_sequence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrokerKvBucket {
    pub name: String,
    pub stream: String,
    pub storage: BrokerStorage,
    /// Backing stream bytes and configured maximum, including retained history.
    pub capacity: BrokerCapacity,
}
