use std::{collections::HashSet, time::Duration};

use async_nats::jetstream::{
    self,
    context::{AccountErrorKind, RequestErrorKind},
    response::Response,
};
use async_trait::async_trait;
use futures_util::{StreamExt, future::join_all, stream};
use rostfrei_nats::{ConnectionHealth, NatsConnection, NatsConnectionConfig, connect};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    sync::OnceCell,
    time::{Instant, timeout, timeout_at},
};

use super::{
    BrokerAccount, BrokerAccountTier, BrokerAccountUsage, BrokerCapacity, BrokerConnectionStatus,
    BrokerConsumer, BrokerConsumerKind, BrokerConsumerLiveness, BrokerInventory, BrokerKvBucket,
    BrokerOperations, BrokerOperationsSnapshot, BrokerReadStatus, BrokerSourceSnapshot,
    BrokerStorage, BrokerStream,
};

const MAX_SOURCES: usize = 8;
const MAX_STREAMS: usize = 100;
const MAX_CONSUMERS_PER_STREAM: usize = 50;
const MAX_CONCURRENT_CONSUMER_LISTS: usize = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_TIMEOUT: Duration = Duration::from_secs(8);

/// Samples explicitly configured `JetStream` accounts without changing broker state.
///
/// Credentials should permit only `$JS.API.INFO`, `$JS.API.STREAM.LIST`,
/// `$JS.API.CONSUMER.LIST.*` (or specific stream names), and inbox replies.
/// Since account and stream inventory are account-wide, use an isolated account
/// if not all metadata may be exposed to the caller.
pub struct NatsBrokerOperations {
    sources: Vec<NatsBrokerSource>,
}

impl NatsBrokerOperations {
    /// Keeps the input order; rejects empty, duplicate, or more than eight sources.
    pub fn new(
        sources: impl IntoIterator<Item = NatsBrokerSource>,
    ) -> Result<Self, NatsBrokerSourceError> {
        let sources: Vec<_> = sources.into_iter().take(MAX_SOURCES + 1).collect();
        if sources.is_empty() || sources.len() > MAX_SOURCES {
            return Err(NatsBrokerSourceError::SourceCount);
        }
        let mut seen = HashSet::new();
        if sources.iter().any(|source| !seen.insert(&source.label)) {
            return Err(NatsBrokerSourceError::DuplicateLabel);
        }
        Ok(Self { sources })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NatsBrokerSourceError {
    #[error("use a public label of 1–64 ASCII letters, digits, spaces, '.', '-' or '_'")]
    InvalidLabel,
    #[error("broker source labels must be unique")]
    DuplicateLabel,
    #[error("configure between one and eight broker sources")]
    SourceCount,
}

pub struct NatsBrokerSource {
    label: String,
    connection: OnceCell<NatsConnection>,
    config: Option<Box<NatsConnectionConfig>>,
}

impl NatsBrokerSource {
    /// Use a caller-owned connection. The adapter does not drain it.
    pub fn connected(
        label: impl Into<String>,
        connection: NatsConnection,
    ) -> Result<Self, NatsBrokerSourceError> {
        Ok(Self {
            label: public_label(label.into())?,
            connection: OnceCell::new_with(Some(connection)),
            config: None,
        })
    }

    /// Connect on demand, retaining the established client for reconnects.
    /// Initial failures do not prevent sampling other sources, and can be retried.
    pub fn configured(
        label: impl Into<String>,
        config: NatsConnectionConfig,
    ) -> Result<Self, NatsBrokerSourceError> {
        Ok(Self {
            label: public_label(label.into())?,
            connection: OnceCell::new(),
            config: Some(Box::new(config)),
        })
    }

    async fn connection(&self) -> Result<&NatsConnection, BrokerConnectionStatus> {
        if let Some(connection) = self.connection.get() {
            return Ok(connection);
        }
        let config = self
            .config
            .as_ref()
            .ok_or(BrokerConnectionStatus::Unavailable)?;
        timeout(
            REQUEST_TIMEOUT,
            self.connection.get_or_try_init(|| connect(config)),
        )
        .await
        .map_err(|_| BrokerConnectionStatus::TimedOut)?
        .map_err(|_| BrokerConnectionStatus::Unavailable)
    }

    async fn sample(&self) -> BrokerSourceSnapshot {
        let deadline = after(SOURCE_TIMEOUT);
        let (status, connection) = match self.connection().await {
            Ok(connection) => (connection_status(connection.health()), Some(connection)),
            Err(status) => (status, None),
        };
        if let Some(connection) = connection.filter(|_| status == BrokerConnectionStatus::Connected)
        {
            let context = connection.jetstream();
            let (account, (streams, kv_buckets)) = tokio::join!(
                sample_account(context, deadline),
                sample_streams(context, deadline),
            );
            BrokerSourceSnapshot {
                label: self.label.clone(),
                connection: status,
                account,
                streams,
                kv_buckets,
            }
        } else {
            let read_status = if status == BrokerConnectionStatus::TimedOut {
                BrokerReadStatus::TimedOut
            } else {
                BrokerReadStatus::Unavailable
            };
            BrokerSourceSnapshot {
                label: self.label.clone(),
                connection: status,
                account: BrokerAccount {
                    status: read_status,
                    usage: None,
                },
                streams: empty_inventory(read_status),
                kv_buckets: empty_inventory(read_status),
            }
        }
    }
}

#[async_trait]
impl BrokerOperations for NatsBrokerOperations {
    async fn snapshot(&self) -> BrokerOperationsSnapshot {
        BrokerOperationsSnapshot {
            sources: join_all(self.sources.iter().map(NatsBrokerSource::sample)).await,
        }
    }
}

fn public_label(label: String) -> Result<String, NatsBrokerSourceError> {
    if label.is_empty()
        || label.len() > 64
        || label.trim() != label
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'-' | b'_'))
    {
        return Err(NatsBrokerSourceError::InvalidLabel);
    }
    Ok(label)
}

const fn connection_status(health: ConnectionHealth) -> BrokerConnectionStatus {
    match health {
        ConnectionHealth::Connecting => BrokerConnectionStatus::Connecting,
        ConnectionHealth::Connected => BrokerConnectionStatus::Connected,
        ConnectionHealth::Disconnected => BrokerConnectionStatus::Disconnected,
        ConnectionHealth::Closed => BrokerConnectionStatus::Closed,
    }
}

const fn empty_inventory<T>(status: BrokerReadStatus) -> BrokerInventory<T> {
    BrokerInventory {
        status,
        items: Vec::new(),
    }
}

fn request_deadline(deadline: Instant) -> Instant {
    deadline.min(after(REQUEST_TIMEOUT))
}

fn after(timeout: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(timeout).unwrap_or(now)
}

async fn sample_account(context: &jetstream::Context, deadline: Instant) -> BrokerAccount {
    let account = match timeout_at(request_deadline(deadline), context.query_account()).await {
        Ok(Ok(account)) => account,
        result => {
            let status = match result {
                Err(_) => BrokerReadStatus::TimedOut,
                Ok(Err(error)) if error.kind() == AccountErrorKind::TimedOut => {
                    BrokerReadStatus::TimedOut
                }
                _ => BrokerReadStatus::Unavailable,
            };
            return BrokerAccount {
                status,
                usage: None,
            };
        }
    };
    let has_tiers = !account.tiers.is_empty();
    let tier_count = account.tiers.len();
    let mut tiers: Vec<_> = account
        .tiers
        .into_iter()
        .filter_map(|(name, tier)| {
            // JetStream replica tier identifiers are R followed by a replica count.
            let replicas = name.strip_prefix('R')?;
            if replicas.is_empty()
                || replicas.len() > 10
                || !replicas.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            Some(BrokerAccountTier {
                name,
                memory: account_capacity(tier.memory, tier.limits.max_memory),
                storage: account_capacity(tier.storage, tier.limits.max_storage),
            })
        })
        .collect();
    tiers.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    tiers.truncate(MAX_STREAMS);
    let status = if tiers.len() == tier_count {
        BrokerReadStatus::Complete
    } else {
        BrokerReadStatus::Truncated
    };
    BrokerAccount {
        status,
        usage: Some(BrokerAccountUsage {
            memory: account_capacity(
                account.memory,
                if has_tiers {
                    None
                } else {
                    account.limits.max_memory
                },
            ),
            storage: account_capacity(
                account.storage,
                if has_tiers {
                    None
                } else {
                    account.limits.max_storage
                },
            ),
            stream_count: account.streams,
            consumer_count: account.consumers,
            tiers,
        }),
    }
}

fn account_capacity(used_bytes: u64, max: Option<i64>) -> BrokerCapacity {
    BrokerCapacity {
        used_bytes,
        limit_bytes: max.and_then(|max| u64::try_from(max).ok()),
    }
}

async fn sample_streams(
    context: &jetstream::Context,
    deadline: Instant,
) -> (
    BrokerInventory<BrokerStream>,
    BrokerInventory<BrokerKvBucket>,
) {
    let streams = list::<jetstream::stream::Info>(context, ListKind::Streams, deadline).await;
    let mut buckets = BrokerInventory {
        status: streams.status,
        items: Vec::new(),
    };
    let mut rows = Vec::with_capacity(streams.items.len());
    for info in streams.items {
        let storage = match info.config.storage {
            jetstream::stream::StorageType::Memory => BrokerStorage::Memory,
            jetstream::stream::StorageType::File => BrokerStorage::File,
        };
        let capacity = account_capacity(info.state.bytes, Some(info.config.max_bytes));
        // Use the same recognition rule as the native KV binding: KV_ prefix,
        // valid bucket ID, and retained history per subject (also fits mirrors).
        if let Some(name) = info.config.name.strip_prefix("KV_").filter(|name| {
            !name.is_empty()
                && info.config.max_messages_per_subject >= 1
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        }) {
            buckets.items.push(BrokerKvBucket {
                name: name.to_owned(),
                stream: info.config.name.clone(),
                storage,
                capacity,
            });
        }
        rows.push(BrokerStream {
            name: info.config.name,
            storage,
            replicas: info.config.num_replicas,
            messages: info.state.messages,
            capacity,
            consumer_count: info.state.consumer_count,
            consumers: empty_inventory(BrokerReadStatus::Unavailable),
        });
    }
    let mut completed: Vec<_> = stream::iter(rows.into_iter().map(|mut row| async move {
        if row.consumer_count == 0 {
            // The broker already reported an empty inventory at stream sample time.
            row.consumers.status = BrokerReadStatus::Complete;
            return row;
        }
        let listed =
            list::<jetstream::consumer::Info>(context, ListKind::Consumers(&row.name), deadline)
                .await;
        row.consumers = BrokerInventory {
            status: listed.status,
            items: listed.items.into_iter().map(consumer_row).collect(),
        };
        row
    }))
    .buffer_unordered(MAX_CONCURRENT_CONSUMER_LISTS)
    .collect()
    .await;
    completed.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    (
        BrokerInventory {
            status: streams.status,
            items: completed,
        },
        buckets,
    )
}

fn consumer_row(info: jetstream::consumer::Info) -> BrokerConsumer {
    let (kind, liveness) = if info.config.deliver_subject.is_some() {
        let liveness = if info.push_bound {
            BrokerConsumerLiveness::PushBound
        } else {
            BrokerConsumerLiveness::PushUnbound
        };
        (BrokerConsumerKind::Push, liveness)
    } else if info.num_waiting > 0 {
        (
            BrokerConsumerKind::Pull,
            BrokerConsumerLiveness::PullWaiting,
        )
    } else {
        (BrokerConsumerKind::Pull, BrokerConsumerLiveness::Unknown)
    };
    BrokerConsumer {
        name: info.name,
        kind,
        durable: info.config.durable_name.is_some(),
        paused: info.paused,
        liveness,
        pending: info.num_pending,
        ack_pending: info.num_ack_pending,
        redelivered: info.num_redelivered,
        waiting: info.num_waiting,
        delivered_stream_sequence: info.delivered.stream_sequence.to_string(),
        ack_floor_stream_sequence: info.ack_floor.stream_sequence.to_string(),
    }
}

#[derive(Clone, Copy)]
enum ListKind<'a> {
    Streams,
    Consumers(&'a str),
}

/// Unlike the convenience iterators in async-nats 0.50, the list envelope
/// retains `missing` and `offline`. In a cluster, an offline member's row can
/// otherwise be absent or appear to have zero usage/lag. Decode rows separately
/// to retain healthy rows even when an offline placeholder has a different shape.
#[derive(Deserialize)]
struct ListPage {
    total: usize,
    offset: usize,
    #[serde(default)]
    missing: Vec<String>,
    #[serde(default)]
    offline: std::collections::HashMap<String, String>,
    streams: Option<Vec<Value>>,
    consumers: Option<Vec<Value>>,
}

trait ListRow: serde::de::DeserializeOwned {
    fn name(&self) -> &str;
    fn applies_to(&self, kind: ListKind<'_>) -> bool;
}

impl ListRow for jetstream::stream::Info {
    fn name(&self) -> &str {
        &self.config.name
    }
    fn applies_to(&self, kind: ListKind<'_>) -> bool {
        matches!(kind, ListKind::Streams)
    }
}

impl ListRow for jetstream::consumer::Info {
    fn name(&self) -> &str {
        &self.name
    }
    fn applies_to(&self, kind: ListKind<'_>) -> bool {
        matches!(kind, ListKind::Consumers(stream) if stream == self.stream_name)
    }
}

fn safe_name(name: &str) -> bool {
    // This name is both public evidence and (for streams) a single subject token.
    // Omit unsupported names instead of transforming identities or emitting controls.
    !name.is_empty()
        && name.len() <= 255
        && name.bytes().all(|byte| {
            byte.is_ascii_graphic() && !matches!(byte, b'.' | b'*' | b'>' | b'/' | b'\\')
        })
}

async fn list<T: ListRow>(
    context: &jetstream::Context,
    kind: ListKind<'_>,
    deadline: Instant,
) -> BrokerInventory<T> {
    let (subject, max) = match kind {
        ListKind::Streams => ("STREAM.LIST".to_owned(), MAX_STREAMS),
        ListKind::Consumers(stream) => {
            (format!("CONSUMER.LIST.{stream}"), MAX_CONSUMERS_PER_STREAM)
        }
    };
    let subject = async_nats::Subject::from(subject);
    let mut items = Vec::<T>::new();
    let mut seen = HashSet::new();
    let mut offset = 0_usize;
    let mut previous_total = None;
    let status = loop {
        if Instant::now() >= deadline {
            break BrokerReadStatus::TimedOut;
        }
        let payload = json!({ "offset": offset });
        let result = timeout_at(
            request_deadline(deadline),
            context.request(subject.clone(), &payload),
        )
        .await;
        let page: ListPage = match result {
            Ok(Ok(Response::Ok(page))) => page,
            Err(_) => break BrokerReadStatus::TimedOut,
            Ok(Err(error)) if error.kind() == RequestErrorKind::TimedOut => {
                break BrokerReadStatus::TimedOut;
            }
            _ => break BrokerReadStatus::Unavailable,
        };
        let Some(page_rows) = (match kind {
            ListKind::Streams => page.streams,
            ListKind::Consumers(_) => page.consumers,
        }) else {
            break BrokerReadStatus::Unavailable;
        };
        let end = offset.saturating_add(page_rows.len());
        let mut incomplete = page.offset != offset
            || previous_total.is_some_and(|total| total != page.total)
            || end > page.total
            || !page.missing.is_empty()
            || !page.offline.is_empty();
        for raw in page_rows.into_iter().take(max.saturating_sub(items.len())) {
            match serde_json::from_value::<T>(raw) {
                Ok(row)
                    if row.applies_to(kind)
                        && safe_name(row.name())
                        && !page.missing.iter().any(|name| name == row.name())
                        && !page.offline.contains_key(row.name())
                        && seen.insert(row.name().to_owned()) =>
                {
                    items.push(row);
                }
                _ => incomplete = true,
            }
        }
        if incomplete || (end == offset && end < page.total) {
            // Native list offsets differ around missing entries between standalone
            // and clustered responses. Do not guess the next offset in this case.
            break BrokerReadStatus::Unavailable;
        }
        if items.len() == max && (end < page.total || end > max) {
            break BrokerReadStatus::Truncated;
        }
        if end == page.total {
            break BrokerReadStatus::Complete;
        }
        offset = end;
        previous_total = Some(page.total);
    };
    items.sort_unstable_by(|a, b| a.name().cmp(b.name()));
    BrokerInventory { status, items }
}
