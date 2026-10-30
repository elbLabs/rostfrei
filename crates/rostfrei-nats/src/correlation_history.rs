use crate::{
    CORRELATION_ID_HEADER, MessagingTopology, NatsEventStoreConfig,
    event_store::decode_consumed_event, integration_subject,
    messaging_config::traffic_subject_prefix,
};
use async_nats::{
    HeaderMap,
    jetstream::{
        self,
        consumer::{self, pull},
        stream::{self, RetentionPolicy},
    },
};
use async_trait::async_trait;
use futures_util::{FutureExt, StreamExt, TryStreamExt};
use rostfrei_core::{
    CorrelationChain, CorrelationHistory, CorrelationRead, CorrelationRef, CorrelationSnapshot,
    CorrelationSourceKind, CorrelationSourceSnapshot, DomainEventRef, EventStoreError,
    EventStoreErrorKind, IntegrationEventRef,
};
use rostfrei_messaging_core::{CorrelationId, MessageTimestamp};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_SOURCES: usize = 32;
const MAX_CORRELATIONS: usize = 256;
const SOURCE_CONCURRENCY: usize = 4;
const MAX_ACTIVE_READS: usize = 4;
const MAX_REFS: usize = 2048;
const MAX_READ_BYTES: usize = 32 * 1024 * 1024;
const API_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(15);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Connecting verifies sources without starting consumers or tasks.
#[derive(Clone)]
pub struct NatsCorrelationHistory {
    context: jetstream::Context,
    sources: Vec<Source>,
    limits: Limits,
    active: Arc<Semaphore>,
}

#[derive(Clone, Copy)]
struct Limits {
    refs: usize,
    bytes: usize,
}

impl NatsCorrelationHistory {
    /// Tighten the per-request bounds (defaults: 2048 references, 32 MiB of
    /// encoded messages plus metadata allowance). A bound marks every requested
    /// chain truncated, including unresolved empty chains.
    pub fn with_limits(
        mut self,
        max_refs: usize,
        max_read_bytes: usize,
    ) -> Result<Self, EventStoreError> {
        if !(1..=MAX_REFS).contains(&max_refs) || !(1..=MAX_READ_BYTES).contains(&max_read_bytes) {
            return Err(invalid(
                "correlation read limits must be positive and no larger than the defaults",
            ));
        }
        self.limits = Limits {
            refs: max_refs,
            bytes: max_read_bytes,
        };
        Ok(self)
    }

    pub async fn connect(
        mut context: jetstream::Context,
        stores: Vec<NatsEventStoreConfig>,
        topology: &MessagingTopology,
    ) -> Result<Self, EventStoreError> {
        if stores.len() >= MAX_SOURCES {
            return Err(invalid("too many correlation history sources"));
        }
        let mut names = BTreeSet::from([topology.integration_event_stream().as_str().to_owned()]);
        for store in &stores {
            if store.application() != topology.application()
                || store.traffic_scope() != topology.traffic_scope()
            {
                return Err(invalid(
                    "correlation sources must share an application and traffic scope",
                ));
            }
            if !names.insert(store.stream_name().to_owned()) {
                return Err(invalid("duplicate correlation source stream"));
            }
        }
        context.set_timeout(API_TIMEOUT);
        let mut sources: Vec<_> = stores
            .into_iter()
            .map(|config| Source::Domain(Box::new(config)))
            .collect();
        sources.push(Source::Integration(Box::new(topology.clone())));
        let reader = Self {
            context,
            sources,
            limits: Limits {
                refs: MAX_REFS,
                bytes: MAX_READ_BYTES,
            },
            active: Arc::new(Semaphore::new(MAX_ACTIVE_READS)),
        };
        tokio::time::timeout(READ_TIMEOUT, reader.capture())
            .await
            .map_err(|_| unavailable("correlation source verification timed out"))??;
        Ok(reader)
    }

    async fn capture(&self) -> Result<Vec<Captured>, EventStoreError> {
        let mut sources: Vec<_> = futures_util::stream::iter(self.sources.iter().cloned())
            .map(|source| async {
                let stream = self
                    .context
                    .get_stream(source.name())
                    .await
                    .map_err(|error| {
                        unavailable(format!("correlation source {}: {error}", source.name()))
                    })?;
                source.validate(stream.cached_info())?;
                Ok(Captured { source, stream })
            })
            .buffer_unordered(SOURCE_CONCURRENCY)
            .try_collect()
            .await?;
        sources.sort_by(|left, right| left.source.name().cmp(right.source.name()));
        Ok(sources)
    }

    async fn read(
        &self,
        ids: BTreeSet<String>,
        permit: Arc<OwnedSemaphorePermit>,
    ) -> Result<CorrelationRead, EventStoreError> {
        // Empty selections must still verify every source and capture its cutoff.
        let sources = self.capture().await?;
        let state = Mutex::new(ReadState::new(&ids, self.limits));
        if !ids.is_empty() {
            let reads: Vec<_> = sources
                .iter()
                .map(|source| {
                    self.read_source(source, &ids, &state, permit.clone())
                        .boxed()
                })
                .collect();
            futures_util::stream::iter(reads)
                .buffer_unordered(SOURCE_CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await?;
        }
        // Do not mix a replacement stream with its captured incarnation, even
        // when the result is empty or truncated.
        let rechecks: Vec<_> = sources
            .iter()
            .map(|source| source.recheck(&self.context).boxed())
            .collect();
        futures_util::stream::iter(rechecks)
            .buffer_unordered(SOURCE_CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?;
        let state = state
            .into_inner()
            .map_err(|_| unavailable("correlation read state poisoned"))?;
        Ok(CorrelationRead {
            chains: state
                .chains
                .into_iter()
                .map(|(correlation_id, mut refs)| {
                    refs.sort_by(|left, right| {
                        left.occurred_at()
                            .cmp(&right.occurred_at())
                            .then_with(|| left.identity().cmp(&right.identity()))
                    });
                    CorrelationChain {
                        correlation_id,
                        refs,
                        truncated: state.truncated,
                    }
                })
                .collect(),
            snapshot: CorrelationSnapshot {
                sources: sources.iter().map(Captured::snapshot).collect(),
            },
        })
    }

    async fn read_source(
        &self,
        source: &Captured,
        ids: &BTreeSet<String>,
        state: &Mutex<ReadState>,
        permit: Arc<OwnedSemaphorePermit>,
    ) -> Result<(), EventStoreError> {
        if lock(state)?.truncated {
            return Ok(());
        }
        let filters = ids.iter().map(|id| source.source.filter(id)).collect();
        let mut replay = Replay::open(self.context.clone(), source, filters, permit).await?;
        let result = Self::collect(source, &mut replay, state).await;
        let cleanup = replay.close().await;
        drop(replay);
        result?;
        cleanup
    }

    async fn collect(
        source: &Captured,
        replay: &mut Replay,
        state: &Mutex<ReadState>,
    ) -> Result<(), EventStoreError> {
        while !lock(state)?.truncated {
            let Some(message) = replay.next().await? else {
                break;
            };
            let bytes = message
                .length
                .checked_add(512)
                .ok_or_else(|| corrupt("correlation message size overflow"))?;
            if !lock(state)?.admit(bytes) {
                break;
            }
            let (id, reference) = source.source.decode(&message)?;
            lock(state)?.insert(id, reference)?;
        }
        Ok(())
    }
}

#[async_trait]
impl CorrelationHistory for NatsCorrelationHistory {
    async fn load_correlations(
        &self,
        ids: &[CorrelationId],
    ) -> Result<CorrelationRead, EventStoreError> {
        if ids.len() > MAX_CORRELATIONS {
            return Err(invalid("too many requested correlations"));
        }
        let permit = self.active.clone().try_acquire_owned().map_err(|_| {
            EventStoreError::new(
                EventStoreErrorKind::CapacityExhausted,
                "too many active correlation reads",
            )
        })?;
        let ids = ids.iter().map(|id| id.as_str().to_owned()).collect();
        tokio::time::timeout(READ_TIMEOUT, self.read(ids, Arc::new(permit)))
            .await
            .map_err(|_| unavailable("correlation history read timed out"))?
    }
}

#[derive(Clone)]
enum Source {
    Domain(Box<NatsEventStoreConfig>),
    Integration(Box<MessagingTopology>),
}

impl Source {
    fn name(&self) -> &str {
        match self {
            Self::Domain(config) => config.stream_name(),
            Self::Integration(topology) => topology.integration_event_stream().as_str(),
        }
    }
    fn filter(&self, id: &str) -> String {
        match self {
            Self::Domain(config) => config.correlation_subject_filter(id),
            Self::Integration(topology) => topology.integration_correlation_filter(id),
        }
    }

    fn validate(&self, info: &stream::Info) -> Result<(), EventStoreError> {
        let config = &info.config;
        if config.name != self.name()
            || config.retention != RetentionPolicy::Limits
            || config.mirror.is_some()
            || config
                .sources
                .as_ref()
                .is_some_and(|sources| !sources.is_empty())
            || config.subject_transform.is_some()
        {
            return Err(mismatch(
                "correlation history requires an untransformed limits-retained source",
            ));
        }
        let mut expected = match self {
            Self::Domain(store) => {
                if !config.max_age.is_zero()
                    || config.allow_message_ttl
                    || !config.deny_delete
                    || !config.deny_purge
                    || config.max_messages != -1
                    || config.max_messages_per_subject != -1
                    || config.discard != stream::DiscardPolicy::New
                    || !config.allow_atomic_publish
                {
                    return Err(mismatch(
                        "domain correlation source must preserve its append-only journal",
                    ));
                }
                store.stream_config().subjects
            }
            Self::Integration(topology) => vec![format!(
                "{}.integration.*.*.>",
                traffic_subject_prefix(topology.application().as_str(), topology.traffic_scope())
            )],
        };
        let mut actual = config.subjects.clone();
        expected.sort();
        actual.sort();
        if expected != actual {
            return Err(mismatch(
                "correlation source subjects do not match the configured scope",
            ));
        }
        Ok(())
    }

    fn decode(
        &self,
        message: &jetstream::Message,
    ) -> Result<(String, CorrelationRef), EventStoreError> {
        match self {
            Self::Domain(config) => {
                let headers = message
                    .headers
                    .as_ref()
                    .ok_or_else(|| corrupt("domain event has no headers"))?;
                let decoded = decode_consumed_event(
                    config,
                    message.subject.as_str(),
                    headers,
                    &message.payload,
                )?;
                let event = decoded.recorded;
                let id = event
                    .correlation_id()
                    .ok_or_else(|| corrupt("filtered domain event has no correlation"))?;
                if one_header(headers, CORRELATION_ID_HEADER)? != id.as_str()
                    || one_header(headers, "Nats-Msg-Id")? != event.event_id().as_str()
                {
                    return Err(corrupt("domain event headers disagree with its envelope"));
                }
                let info = message
                    .info()
                    .map_err(|_| corrupt("invalid domain publication metadata"))?;
                let milliseconds = info
                    .published
                    .unix_timestamp_nanos()
                    .checked_div(1_000_000)
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| corrupt("invalid domain publication timestamp"))?;
                let occurred_at = MessageTimestamp::from_unix_milliseconds(milliseconds)
                    .map_err(|_| corrupt("invalid domain publication timestamp"))?;
                Ok((
                    id.as_str().to_owned(),
                    CorrelationRef::Domain(DomainEventRef::from_recorded(
                        config.bounded_context().as_str(),
                        &event,
                        occurred_at,
                    )),
                ))
            }
            Self::Integration(topology) => {
                let (subject, envelope) = integration_subject::decode_message(
                    message.subject.as_str(),
                    message.headers.as_ref(),
                    &message.payload,
                )
                .map_err(|_| corrupt("integration subject, headers and envelope must agree"))?;
                let address = subject.address();
                if address.application() != topology.application().as_str()
                    || address.traffic_scope() != topology.traffic_scope()
                    || subject.as_str() != message.subject.as_str()
                {
                    return Err(corrupt("integration event belongs to another source scope"));
                }
                Ok((
                    envelope.correlation_id().as_str().to_owned(),
                    CorrelationRef::Integration(IntegrationEventRef {
                        context: address.context().to_owned(),
                        subject: message.subject.to_string(),
                        message_id: envelope.message_id().as_str().to_owned(),
                        event_type: address.name().to_owned(),
                        schema_version: envelope.schema_version().get(),
                        causation_id: envelope.causation_id().map(|id| id.as_str().to_owned()),
                        occurred_at: envelope.occurred_at(),
                    }),
                ))
            }
        }
    }
}

struct Captured {
    source: Source,
    stream: stream::Stream,
}
impl Captured {
    fn snapshot(&self) -> CorrelationSourceSnapshot {
        let (context, kind) = match &self.source {
            Source::Domain(config) => (
                Some(config.bounded_context().as_str().to_owned()),
                CorrelationSourceKind::Domain,
            ),
            Source::Integration(_) => (None, CorrelationSourceKind::Integration),
        };
        CorrelationSourceSnapshot {
            context,
            kind,
            stream: self.source.name().to_owned(),
            cutoff_sequence: self.stream.cached_info().state.last_sequence.to_string(),
        }
    }
    async fn recheck(&self, context: &jetstream::Context) -> Result<(), EventStoreError> {
        let current = context
            .get_stream(self.source.name())
            .await
            .map_err(|error| unavailable(format!("correlation source recheck failed: {error}")))?;
        let current = current.cached_info();
        let captured = self.stream.cached_info();
        if current.created != captured.created
            || current.config != captured.config
            || current.state.last_sequence < captured.state.last_sequence
        {
            return Err(unavailable("correlation source changed during the read"));
        }
        Ok(())
    }
}

struct ReadState {
    chains: BTreeMap<String, Vec<CorrelationRef>>,
    identities: BTreeMap<(String, String, String), (String, usize)>,
    count: usize,
    bytes: usize,
    truncated: bool,
    limits: Limits,
}
impl ReadState {
    fn new(ids: &BTreeSet<String>, limits: Limits) -> Self {
        Self {
            chains: ids.iter().map(|id| (id.clone(), Vec::new())).collect(),
            identities: BTreeMap::new(),
            count: 0,
            bytes: 0,
            truncated: false,
            limits,
        }
    }
    fn admit(&mut self, bytes: usize) -> bool {
        let total = self.bytes.checked_add(bytes);
        if self.count >= self.limits.refs || total.is_none_or(|total| total > self.limits.bytes) {
            self.truncated = true;
            return false;
        }
        self.bytes = total.unwrap_or(self.limits.bytes);
        true
    }
    fn insert(&mut self, id: String, reference: CorrelationRef) -> Result<(), EventStoreError> {
        let (kind, context, event) = reference.identity();
        let identity = (kind.to_owned(), context.to_owned(), event.to_owned());
        if let Some((prior_id, index)) = self.identities.get(&identity) {
            let prior = self
                .chains
                .get(prior_id)
                .and_then(|chain| chain.get(*index));
            if prior_id != &id || prior != Some(&reference) {
                return Err(corrupt("conflicting retained correlation event identity"));
            }
            return Ok(());
        }
        let chain = self
            .chains
            .get_mut(&id)
            .ok_or_else(|| corrupt("native filter returned an unrequested correlation"))?;
        self.identities.insert(identity, (id, chain.len()));
        chain.push(reference);
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| corrupt("correlation reference count overflow"))?;
        Ok(())
    }
}
fn lock(state: &Mutex<ReadState>) -> Result<std::sync::MutexGuard<'_, ReadState>, EventStoreError> {
    state
        .lock()
        .map_err(|_| unavailable("correlation read state poisoned"))
}

struct Replay {
    lease: ConsumerLease,
    messages: Option<pull::Ordered>,
    stream: stream::Stream,
    filters: Vec<String>,
    created: String,
    remaining: u64,
    cutoff: u64,
    last_sequence: u64,
    last_consumer_sequence: u64,
}
impl Replay {
    async fn open(
        context: jetstream::Context,
        source: &Captured,
        filters: Vec<String>,
        permit: Arc<OwnedSemaphorePermit>,
    ) -> Result<Self, EventStoreError> {
        let name = context.client().new_inbox().replace('.', "_");
        let stream = source.stream.clone();
        let wanted = filters.clone();
        // Own creation until its response, even if the caller is cancelled:
        // deleting a not-yet-created consumer can otherwise orphan it.
        let creation = tokio::spawn(async move {
            let lease = ConsumerLease {
                context,
                stream: stream.cached_info().config.name.clone(),
                name: Some(name.clone()),
                permit,
            };
            let consumer = stream
                .create_consumer(pull::OrderedConfig {
                    name: Some(name),
                    filter_subjects: wanted,
                    deliver_policy: consumer::DeliverPolicy::All,
                    replay_policy: consumer::ReplayPolicy::Instant,
                    ..Default::default()
                })
                .await
                .map_err(|error| {
                    unavailable(format!("cannot create correlation replay: {error}"))
                })?;
            Ok::<_, EventStoreError>((lease, consumer))
        });
        let (lease, consumer) = creation
            .await
            .map_err(|_| unavailable("correlation consumer creation task failed"))??;
        validate_consumer(consumer.cached_info(), &lease, &filters)?;
        if consumer.cached_info().config.deliver_policy != consumer::DeliverPolicy::All
            || consumer.cached_info().delivered.consumer_sequence != 0
        {
            return Err(corrupt(
                "correlation replay did not start with the complete retained selection",
            ));
        }
        let remaining = consumer.cached_info().num_pending;
        let created = consumer.cached_info().created.to_string();
        // async-nats 0.50 ordered pulls own 500-message batching/recovery and have
        // no batch-byte tuning. Our byte bound covers examined messages and
        // reference state, not the client's transport prefetch buffers.
        let messages =
            if remaining == 0 {
                None
            } else {
                Some(consumer.messages().await.map_err(|error| {
                    unavailable(format!("cannot open correlation replay: {error}"))
                })?)
            };
        Ok(Self {
            lease,
            messages,
            stream: source.stream.clone(),
            filters,
            created,
            remaining,
            cutoff: source.stream.cached_info().state.last_sequence,
            last_sequence: 0,
            last_consumer_sequence: 0,
        })
    }
    async fn next(&mut self) -> Result<Option<jetstream::Message>, EventStoreError> {
        if self.remaining == 0 {
            return Ok(None);
        }
        tokio::time::timeout(API_TIMEOUT, self.next_proven())
            .await
            .map_err(|_| unavailable("correlation replay stalled before proving completion"))?
    }
    async fn next_proven(&mut self) -> Result<Option<jetstream::Message>, EventStoreError> {
        loop {
            let messages = self
                .messages
                .as_mut()
                .ok_or_else(|| unavailable("correlation replay is absent"))?;
            tokio::select! {
                message = messages.next() => {
                    let message = message.ok_or_else(|| unavailable("correlation replay ended before its cutoff"))?
                        .map_err(|error| unavailable(format!("correlation replay failed: {error}")))?;
                    let info = message.info().map_err(|_| corrupt("invalid correlation delivery metadata"))?;
                    if info.stream != self.lease.stream || Some(info.consumer) != self.lease.name.as_deref() || info.stream_sequence <= self.last_sequence {
                        return Err(corrupt("correlation delivery identity or ordering mismatch"));
                    }
                    self.last_sequence = info.stream_sequence;
                    self.last_consumer_sequence = info.consumer_sequence;
                    if info.stream_sequence > self.cutoff { self.remaining = 0; return Ok(None); }
                    self.remaining = self.remaining.checked_sub(1).ok_or_else(|| corrupt("correlation replay count overflow"))?;
                    // Expiry can reduce the initial matching count. This is
                    // broker evidence; silence or a timeout is not completion.
                    if info.pending == 0 { self.remaining = 0; }
                    return Ok(Some(message));
                }
                () = tokio::time::sleep(PROGRESS_INTERVAL) => {
                    let name = self.lease.name.as_deref().ok_or_else(|| unavailable("correlation replay is closed"))?;
                    match self.stream.consumer_info(name).await {
                        Ok(info) => {
                            validate_consumer(&info, &self.lease, &self.filters)?;
                            // Pending excludes messages in client buffers, so
                            // require delivery progress to agree as well.
                            let received_all = info.delivered.stream_sequence == self.last_sequence
                                || (info.created.to_string() == self.created && info.delivered.consumer_sequence == self.last_consumer_sequence);
                            if info.num_pending == 0 && received_all { self.remaining = 0; return Ok(None); }
                        }
                        Err(error) if matches!(error.kind(), jetstream::context::ConsumerInfoErrorKind::JetStream(error)
                            if error.error_code() == jetstream::ErrorCode::CONSUMER_NOT_FOUND) => (), // Native recovery.
                        Err(error) => return Err(unavailable(format!("cannot verify correlation replay progress: {error}"))),
                    }
                }
            }
        }
    }
    async fn close(&mut self) -> Result<(), EventStoreError> {
        self.messages.take();
        self.lease.close().await
    }
}
// Stop native pulls/recovery before releasing the named broker consumer.
impl Drop for Replay {
    fn drop(&mut self) {
        self.messages.take();
    }
}

fn validate_consumer(
    info: &consumer::Info,
    lease: &ConsumerLease,
    filters: &[String],
) -> Result<(), EventStoreError> {
    let actual: BTreeSet<_> = info.config.filter_subjects.iter().collect();
    let expected: BTreeSet<_> = filters.iter().collect();
    if info.stream_name != lease.stream
        || Some(info.name.as_str()) != lease.name.as_deref()
        || actual != expected
        || !info.config.filter_subject.is_empty()
        || info.config.durable_name.is_some()
        || info.config.ack_policy != consumer::AckPolicy::None
        || info.config.headers_only
        || info.config.deliver_subject.is_some()
    {
        return Err(corrupt(
            "correlation replay consumer does not match its requested identity and filters",
        ));
    }
    Ok(())
}

struct ConsumerLease {
    context: jetstream::Context,
    stream: String,
    name: Option<String>,
    permit: Arc<OwnedSemaphorePermit>,
}
impl ConsumerLease {
    async fn close(&mut self) -> Result<(), EventStoreError> {
        if let Some(name) = &self.name {
            delete_consumer(&self.context, &self.stream, name).await?;
            self.name = None;
        }
        Ok(())
    }
}
impl Drop for ConsumerLease {
    fn drop(&mut self) {
        if let Some(name) = self.name.take() {
            let context = self.context.clone();
            let stream = self.stream.clone();
            let permit = self.permit.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    // Keep the request slot until cancellation cleanup finishes.
                    let _permit = permit;
                    if let Err(error) = delete_consumer(&context, &stream, &name).await {
                        tracing::warn!(%error, "failed to clean up cancelled correlation replay");
                        // Native ordered consumers also expire after 30s idle.
                    }
                });
            }
        }
    }
}
async fn delete_consumer(
    context: &jetstream::Context,
    stream: &str,
    name: &str,
) -> Result<(), EventStoreError> {
    match context.delete_consumer_from_stream(name, stream).await {
        Ok(_) => Ok(()),
        Err(error) if consumer_missing(&error) => Ok(()),
        Err(error) => Err(unavailable(format!(
            "cannot clean up correlation replay: {error}"
        ))),
    }
}
fn consumer_missing(error: &stream::ConsumerError) -> bool {
    matches!(error.kind(), stream::ConsumerErrorKind::JetStream(error)
        if matches!(error.error_code(), jetstream::ErrorCode::CONSUMER_NOT_FOUND | jetstream::ErrorCode::STREAM_NOT_FOUND))
}
fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, EventStoreError> {
    let mut values = headers.get_all(name.to_owned());
    let value = values
        .next()
        .ok_or_else(|| corrupt("missing domain correlation header"))?;
    if values.next().is_some() {
        return Err(corrupt("ambiguous domain correlation header"));
    }
    Ok(value.as_str())
}
fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}
fn mismatch(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::ConfigurationMismatch, message)
}
fn corrupt(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}
fn unavailable(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::Unavailable, message)
}
