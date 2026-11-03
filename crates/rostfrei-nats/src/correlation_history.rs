use crate::{
    MessagingTopology, NatsEventStoreConfig, event_store::receipt_inspection, integration_subject,
    messaging_config::traffic_subject_prefix,
};
use async_nats::jetstream::{
    self,
    consumer::{self, pull},
    stream::{self, RetentionPolicy},
};
use async_trait::async_trait;
use futures_util::{FutureExt, StreamExt, TryStreamExt};
use rostfrei_core::{
    CorrelationChain, CorrelationHistory, CorrelationRead, CorrelationRef, CorrelationSnapshot,
    CorrelationSourceKind, CorrelationSourceSnapshot, EventStoreError, EventStoreErrorKind,
    IntegrationEventRef,
};
use rostfrei_messaging_core::CorrelationId;
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
    domain_attachments: BTreeMap<String, Attachment>,
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
        let mut reader = Self {
            context,
            sources,
            domain_attachments: BTreeMap::new(),
            limits: Limits {
                refs: MAX_REFS,
                bytes: MAX_READ_BYTES,
            },
            active: Arc::new(Semaphore::new(MAX_ACTIVE_READS)),
        };
        let sources = tokio::time::timeout(READ_TIMEOUT, reader.capture())
            .await
            .map_err(|_| unavailable("correlation source verification timed out"))??;
        reader.domain_attachments = sources
            .into_iter()
            .filter(|source| matches!(source.source, Source::Domain(_)))
            .map(|source| {
                (
                    source.source.name().to_owned(),
                    Attachment::from_info(source.stream.cached_info()),
                )
            })
            .collect();
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
                if let Some(attachment) = self.domain_attachments.get(source.name()) {
                    attachment.verify(stream.cached_info())?;
                }
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
        let mut filters: Vec<_> = ids
            .iter()
            .flat_map(|id| source.source.filters(id))
            .collect();
        let Some(witness) = source.witness(&self.context, state).await? else {
            return Ok(());
        };
        // Generated selectors have only single-token wildcards. Do not add an
        // overlapping exact filter when the witness is already selected.
        let witness_only = if selection_covers(&filters, &witness) {
            None
        } else {
            filters.push(witness.clone());
            Some(witness)
        };
        let mut replay =
            Replay::open(self.context.clone(), source, filters, witness_only, permit).await?;
        let result = Self::collect(source, &mut replay, state).await;
        let cleanup = replay.close().await;
        drop(replay);
        if result.is_err() {
            // A replacement can first appear as a missing or incompatible raw
            // event. Preserve the source-incarnation error in that case.
            source.recheck(&self.context).await?;
        }
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
            if message
                .info()
                .map_err(|_| corrupt("invalid correlation delivery metadata"))?
                .stream_sequence
                > source.stream.cached_info().state.last_sequence
            {
                break; // Above-cutoff overfetch still consumed examined-data budget.
            }
            if replay.witness_only.as_deref() == Some(message.subject.as_str()) {
                continue; // Exact-subject overfetch is charged but is not selected evidence.
            }
            match &source.source {
                Source::Domain(config) => {
                    let Some((id, references)) =
                        receipt_inspection::inspect(config, &source.stream, &message, |bytes| {
                            Ok(lock(state)?.admit(bytes))
                        })
                        .await?
                    else {
                        break;
                    };
                    if !lock(state)?.chains.contains_key(&id) {
                        return Err(corrupt(
                            "native receipt filter returned an unrequested correlation",
                        ));
                    }
                    for reference in references {
                        lock(state)?.insert(id.clone(), CorrelationRef::Domain(reference))?;
                        if lock(state)?.truncated {
                            break;
                        }
                    }
                }
                Source::Integration(topology) => {
                    let (id, reference) = decode_integration(topology, &message)?;
                    lock(state)?.insert(id, reference)?;
                }
            }
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
    fn filters(&self, id: &str) -> Vec<String> {
        match self {
            Self::Domain(config) => config.correlation_receipt_subject_filters(id).to_vec(),
            Self::Integration(topology) => vec![topology.integration_correlation_filter(id)],
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
                receipt_inspection::validate_authority(store, info)?;
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
}

fn decode_integration(
    topology: &MessagingTopology,
    message: &jetstream::Message,
) -> Result<(String, CorrelationRef), EventStoreError> {
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

#[derive(Clone)]
struct Attachment {
    created: time::OffsetDateTime,
    config: stream::Config,
}
impl Attachment {
    fn from_info(info: &stream::Info) -> Self {
        Self {
            created: info.created,
            config: info.config.clone(),
        }
    }
    fn verify(&self, info: &stream::Info) -> Result<(), EventStoreError> {
        if self.created != info.created || self.config != info.config {
            return Err(unavailable(
                "attached domain correlation source was replaced or reconfigured; reconnect its history readers",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
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
    async fn current(&self, context: &jetstream::Context) -> Result<stream::Info, EventStoreError> {
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
        Ok(current.clone())
    }
    async fn recheck(&self, context: &jetstream::Context) -> Result<(), EventStoreError> {
        self.current(context).await.map(|_| ())
    }
    async fn expired_through_cutoff(
        &self,
        context: &jetstream::Context,
    ) -> Result<bool, EventStoreError> {
        if matches!(self.source, Source::Domain(_)) {
            return Ok(false);
        }
        let current = self.current(context).await?;
        Ok(current.state.messages == 0
            || current.state.first_sequence > self.stream.cached_info().state.last_sequence)
    }
    async fn witness(
        &self,
        context: &jetstream::Context,
        state: &Mutex<ReadState>,
    ) -> Result<Option<String>, EventStoreError> {
        let captured = self.stream.cached_info();
        if captured.state.messages == 0 || !lock(state)?.admit(0) {
            return Ok(None);
        }
        let raw = match self
            .stream
            .get_raw_message(captured.state.last_sequence)
            .await
        {
            Ok(raw) => raw,
            Err(error) => {
                if error.kind() == stream::RawMessageErrorKind::NoMessageFound
                    && self.expired_through_cutoff(context).await?
                {
                    return Ok(None);
                }
                return Err(unavailable(format!(
                    "cannot obtain retained correlation cutoff witness: {error}"
                )));
            }
        };
        if !lock(state)?.admit(receipt_inspection::raw_examined_bytes(&raw)?) {
            return Ok(None);
        }
        if raw.sequence != captured.state.last_sequence {
            return Err(corrupt(
                "cutoff witness lookup returned a different sequence",
            ));
        }
        Ok(Some(raw.subject.to_string()))
    }
}

fn selection_covers(filters: &[String], subject: &str) -> bool {
    filters.iter().any(|filter| {
        let mut tokens = subject.split('.');
        filter.split('.').all(|pattern| {
            tokens
                .next()
                .is_some_and(|token| pattern == "*" || pattern == token)
        }) && tokens.next().is_none()
    })
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
        if self.truncated
            || self.count >= self.limits.refs
            || total.is_none_or(|total| total > self.limits.bytes)
        {
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
        if self.count >= self.limits.refs {
            self.truncated = true;
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
    source: Captured,
    filters: Vec<String>,
    witness_only: Option<String>,
    complete: bool,
    cutoff: u64,
    last_sequence: u64,
}
impl Replay {
    async fn open(
        context: jetstream::Context,
        source: &Captured,
        filters: Vec<String>,
        witness_only: Option<String>,
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
        // async-nats 0.50 ordered pulls own 500-message batching/recovery and have
        // no batch-byte tuning. Our byte bound covers examined messages and
        // reference state, not the client's transport prefetch buffers.
        // Native gap: OrderedConfig fixes replicas=1, so its consumer can run on
        // a lagging stream follower. No pending count proves leader-cutoff coverage.
        // MSG.GET supplied a retained witness at C and its exact subject is in the
        // filters. Ordered delivery at C (or later) proves traversal: NATS applies
        // stream records contiguously; native recovery resumes after the last
        // yielded sequence with these same filters. A fresh leader INFO can only
        // substitute when every record <= C has expired, never just pending=0.
        let messages =
            Some(consumer.messages().await.map_err(|error| {
                unavailable(format!("cannot open correlation replay: {error}"))
            })?);
        Ok(Self {
            lease,
            messages,
            source: source.clone(),
            filters,
            witness_only,
            complete: false,
            cutoff: source.stream.cached_info().state.last_sequence,
            last_sequence: 0,
        })
    }
    async fn next(&mut self) -> Result<Option<jetstream::Message>, EventStoreError> {
        if self.complete {
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
                    self.complete = info.stream_sequence >= self.cutoff;
                    return Ok(Some(message));
                }
                () = tokio::time::sleep(PROGRESS_INTERVAL) => {
                    let name = self.lease.name.as_deref().ok_or_else(|| unavailable("correlation replay is closed"))?;
                    match self.source.stream.consumer_info(name).await {
                        Ok(info) => {
                            validate_consumer(&info, &self.lease, &self.filters)?;
                        }
                        Err(error) if matches!(error.kind(), jetstream::context::ConsumerInfoErrorKind::JetStream(error)
                            if error.error_code() == jetstream::ErrorCode::CONSUMER_NOT_FOUND) => (), // Native recovery.
                        Err(error) => return Err(unavailable(format!("cannot verify correlation replay progress: {error}"))),
                    }
                    if self.source.expired_through_cutoff(&self.lease.context).await? {
                        self.complete = true;
                        return Ok(None);
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
