use std::{
    collections::{BTreeMap, HashSet},
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};

use async_nats::jetstream::{
    self,
    consumer::{self, AckPolicy, DeliverPolicy},
    stream::{LastRawMessageErrorKind, RawMessageErrorKind, RetentionPolicy},
};
use async_trait::async_trait;
use futures_util::StreamExt as _;
use rostfrei_core::{
    CORRELATION_REFS_PER_PAGE, CorrelationChain, CorrelationIndexEntry, CorrelationIndexFreshness,
    CorrelationIndexReader, CorrelationRef, CorrelationSourceFreshness, CorrelationSourceKind,
    CorrelationSourceState, DomainEventRef, IntegrationEventRef, JsonReadModelCodec,
    MAX_CORRELATION_PAGES, ReadModelEntry, ReadModelError, ReadModelErrorKind, ReadModelKey,
    ReadModelRevision, ReadModelStore,
};
use rostfrei_messaging_core::{
    ApplicationName, CausationId, CorrelationId, IntegrationEventAddress, MessageId,
    MessageTimestamp, SchemaVersion, TrafficScope,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    MessagingTopology, NatsEventStoreConfig, NatsReadModelConfig, NatsReadModelStore,
    event_store::decode_consumed_event, messaging_config::traffic_subject_prefix,
};

pub const CORRELATION_INDEX_NAME: &str = "correlation-index";
/// The index grows linearly with every event store of the application.
pub const DEFAULT_CORRELATION_INDEX_MAX_BYTES: i64 = 1024 * 1024 * 1024;
const MAX_SOURCES: usize = 256;
const MAX_BATCH_MESSAGES: usize = 256;
const BATCH_LINGER: Duration = Duration::from_millis(20);
const MAX_WRITE_ATTEMPTS: usize = 8;
const RETRY_DELAY: Duration = Duration::from_millis(500);
const LAG_CURSOR_INACTIVE_THRESHOLD: Duration = Duration::from_secs(30);
const SOURCES_KEY: &str = "sources";
/// Projector consumers are named `correlation-index--<kind>--<context>--<uuid>`;
/// readers treat a source as active while such a consumer exists.
const PROJECTOR_CONSUMER_PREFIX: &str = "correlation-index";
const LITERAL_KEY_PREFIX: &str = "correlation.";
const DIGEST_KEY_PREFIX: &str = "correlation-digest.";
const MAX_LITERAL_KEY_BYTES: usize = 240;

type Store<T> = NatsReadModelStore<T, JsonReadModelCodec<T>>;

impl NatsReadModelConfig {
    /// Application-scoped bucket for the framework-owned correlation index.
    /// Raise `max_bytes` with `update_read_model` before it is exhausted.
    pub fn correlation_index(
        application: &ApplicationName,
        traffic_scope: TrafficScope,
    ) -> Result<Self, ReadModelError> {
        Self::application_scoped(application, traffic_scope, CORRELATION_INDEX_NAME)?
            .with_storage_limits(DEFAULT_CORRELATION_INDEX_MAX_BYTES, 256 * 1024)
    }
}

/// Projection progress for one source stream, stored in the index bucket so
/// that deleting the bucket also resets every checkpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexSource {
    context: String,
    kind: CorrelationSourceKind,
    stream: String,
    filter_subject: String,
    /// Stream `created` time written by earlier versions. `JetStream` rewrites it
    /// on server restart after a stream update, so it is read and ignored.
    #[serde(default, skip_serializing)]
    generation: Option<String>,
    checkpoint: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    skipped: u64,
}

/// Retained range of a source stream, from its stream state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamRange {
    first_sequence: u64,
    last_sequence: u64,
}

/// Where a source's checkpoint lies relative to the retained stream range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourcePosition {
    Consistent,
    /// The stream no longer retains this many sequences after the checkpoint.
    Gap(u64),
    /// The stream's last sequence lies before the checkpoint: sequences never
    /// regress, so the stream was deleted and created again.
    Recreated,
}

impl SourcePosition {
    /// A Limits stream that removed messages after the checkpoint, by age, size
    /// or purge, keeps its last sequence and is a gap, not a recreation. A
    /// stream recreated with at least as many messages as the checkpoint is not
    /// detectable from sequences.
    const fn classify(checkpoint: u64, range: StreamRange) -> Self {
        if range.last_sequence < checkpoint {
            Self::Recreated
        } else if checkpoint > 0 && range.first_sequence > checkpoint.saturating_add(1) {
            Self::Gap(
                range
                    .first_sequence
                    .saturating_sub(checkpoint)
                    .saturating_sub(1),
            )
        } else {
            Self::Consistent
        }
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes skipped fields by reference"
)]
const fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn recreated() -> ReadModelError {
    ReadModelError::new(
        ReadModelErrorKind::ConfigurationMismatch,
        "source stream was recreated; delete and rebuild the correlation index",
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDirectory {
    sources: Vec<String>,
}

#[derive(Deserialize)]
struct IntegrationMetadata {
    message_id: MessageId,
    schema_version: SchemaVersion,
    occurred_at: MessageTimestamp,
    correlation_id: CorrelationId,
    #[serde(default)]
    causation_id: Option<CausationId>,
}

struct IndexStores {
    entries: Store<CorrelationIndexEntry>,
    sources: Store<IndexSource>,
    directory: Store<SourceDirectory>,
}

impl IndexStores {
    async fn connect(
        context: &jetstream::Context,
        config: &NatsReadModelConfig,
    ) -> Result<Self, ReadModelError> {
        if config.context().is_some() || config.name() != CORRELATION_INDEX_NAME {
            return Err(ReadModelError::new(
                ReadModelErrorKind::ConfigurationMismatch,
                "correlation index requires its application-scoped read-model policy",
            ));
        }
        Ok(Self {
            entries: open(context, config).await?,
            sources: open(context, config).await?,
            directory: open(context, config).await?,
        })
    }

    async fn pages(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<Vec<(ReadModelKey, Option<ReadModelEntry<CorrelationIndexEntry>>)>, ReadModelError>
    {
        let base = correlation_key(correlation_id);
        let mut pages = Vec::new();
        for page in 0..MAX_CORRELATION_PAGES {
            let key = page_key(&base, page)?;
            let entry = self.entries.read(&key).await?;
            let full = entry
                .as_ref()
                .is_some_and(|entry| entry.value.refs.len() >= CORRELATION_REFS_PER_PAGE);
            pages.push((key, entry));
            if !full {
                break;
            }
        }
        Ok(pages)
    }

    async fn append(
        &self,
        correlation_id: &CorrelationId,
        references: &[CorrelationRef],
    ) -> Result<(), ReadModelError> {
        for _ in 0..MAX_WRITE_ATTEMPTS {
            let mut pages = self.pages(correlation_id).await?;
            let mut known = HashSet::new();
            for (_, entry) in &pages {
                for reference in entry.iter().flat_map(|entry| &entry.value.refs) {
                    known.insert(reference.identity());
                }
            }
            let fresh: Vec<_> = references
                .iter()
                .filter(|reference| known.insert(reference.identity()))
                .cloned()
                .collect();
            if fresh.is_empty() {
                return Ok(());
            }
            let Some((key, last)) = pages.pop() else {
                return Err(ReadModelError::new(
                    ReadModelErrorKind::InvalidData,
                    "correlation index has no first page",
                ));
            };
            let writes = page_writes(correlation_id, pages.len(), key, last, fresh)?;
            match self.write_pages(writes).await {
                Err(error) if error.kind() == ReadModelErrorKind::Conflict => {}
                result => return result,
            }
        }
        Err(contention())
    }

    async fn write_pages(&self, writes: Vec<PageWrite>) -> Result<(), ReadModelError> {
        for write in writes {
            match &write.revision {
                Some(revision) => {
                    self.entries
                        .update(&write.key, revision, &write.page)
                        .await?
                }
                None => self.entries.create(&write.key, &write.page).await?,
            };
        }
        Ok(())
    }

    async fn chain(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<CorrelationChain, ReadModelError> {
        let mut chain = CorrelationChain::default();
        for (_, entry) in self.pages(correlation_id).await? {
            let Some(entry) = entry else { break };
            chain.truncated |= entry.value.truncated;
            chain.refs.extend(entry.value.refs);
        }
        chain.refs.sort_by_key(CorrelationRef::occurred_at);
        Ok(chain)
    }

    async fn source(&self, key: &ReadModelKey) -> Result<Option<IndexSource>, ReadModelError> {
        Ok(self.sources.read(key).await?.map(|entry| entry.value))
    }

    /// Registers the source or resumes it. A gap after the checkpoint is
    /// recorded as `skipped` and the checkpoint moves to the first retained
    /// sequence; a recreated stream is refused.
    async fn register(
        &self,
        source: &IndexSource,
        range: StreamRange,
    ) -> Result<u64, ReadModelError> {
        let key = source_key(source.kind, &source.context)?;
        let checkpoint = cas(&self.sources, &key, |stored| {
            let Some(stored) = stored else {
                return Ok(Some(source.clone()));
            };
            if stored.stream != source.stream || stored.filter_subject != source.filter_subject {
                return Err(ReadModelError::new(
                    ReadModelErrorKind::ConfigurationMismatch,
                    "source stream policy changed; delete and rebuild the correlation index",
                ));
            }
            match SourcePosition::classify(stored.checkpoint, range) {
                SourcePosition::Recreated => Err(recreated()),
                SourcePosition::Gap(skipped) => {
                    tracing::warn!(stream = %stored.stream, context = %stored.context,
                        checkpoint = stored.checkpoint, skipped,
                        "correlation index source lost messages before they were indexed");
                    Ok(Some(IndexSource {
                        checkpoint: range.first_sequence.saturating_sub(1),
                        skipped: stored.skipped.saturating_add(skipped),
                        generation: None,
                        ..stored.clone()
                    }))
                }
                SourcePosition::Consistent => Ok(None),
            }
        })
        .await?
        .checkpoint;
        cas(
            &self.directory,
            &ReadModelKey::new(SOURCES_KEY)?,
            |stored| {
                let mut directory = stored.cloned().unwrap_or_default();
                if directory.sources.iter().any(|name| name == key.as_str()) {
                    return Ok(None);
                }
                if directory.sources.len() >= MAX_SOURCES {
                    return Err(ReadModelError::new(
                        ReadModelErrorKind::CapacityExhausted,
                        "correlation index source directory is full",
                    ));
                }
                directory.sources.push(key.as_str().to_owned());
                Ok(Some(directory))
            },
        )
        .await?;
        Ok(checkpoint)
    }

    async fn advance(
        &self,
        kind: CorrelationSourceKind,
        context: &str,
        checkpoint: u64,
    ) -> Result<(), ReadModelError> {
        let key = source_key(kind, context)?;
        cas(&self.sources, &key, |stored| {
            let stored = stored.ok_or_else(|| {
                ReadModelError::new(
                    ReadModelErrorKind::InvalidData,
                    "correlation index source disappeared; restart the projector",
                )
            })?;
            Ok((stored.checkpoint < checkpoint).then(|| IndexSource {
                checkpoint,
                ..stored.clone()
            }))
        })
        .await
        .map(|_| ())
    }
}

struct PageWrite {
    key: ReadModelKey,
    revision: Option<ReadModelRevision>,
    page: CorrelationIndexEntry,
}

fn page_writes(
    correlation_id: &CorrelationId,
    mut index: usize,
    key: ReadModelKey,
    last: Option<ReadModelEntry<CorrelationIndexEntry>>,
    fresh: Vec<CorrelationRef>,
) -> Result<Vec<PageWrite>, ReadModelError> {
    let base = correlation_key(correlation_id);
    let (mut page, mut revision) = last.map_or_else(
        || (CorrelationIndexEntry::default(), None),
        |entry| (entry.value, Some(entry.revision)),
    );
    let mut key = key;
    let mut writes = Vec::new();
    let mut changed = false;
    let mut omitted = 0_usize;
    for reference in fresh {
        if page.refs.len() >= CORRELATION_REFS_PER_PAGE {
            let next = index
                .checked_add(1)
                .filter(|next| *next < MAX_CORRELATION_PAGES);
            let Some(next) = next else {
                changed |= !page.truncated;
                page.truncated = true;
                omitted = omitted.saturating_add(1);
                continue;
            };
            writes.push(PageWrite {
                key,
                revision: revision.take(),
                page: std::mem::take(&mut page),
            });
            index = next;
            key = page_key(&base, index)?;
        }
        page.refs.push(reference);
        changed = true;
    }
    if omitted > 0 {
        tracing::warn!(correlation_id = %correlation_id, omitted,
            "correlation exceeds its reference cap; further references are counted as truncated");
    }
    if changed {
        writes.push(PageWrite {
            key,
            revision,
            page,
        });
    }
    Ok(writes)
}

/// Projects one bounded context's committed domain events and the integration
/// events it publishes into the application-scoped correlation index.
///
/// Only envelope metadata is read; payloads are never decoded. Progress is
/// stored in the index bucket, so a deleted and re-provisioned bucket is rebuilt
/// from the start of each source stream on the next run.
///
/// Each source is read through an ordered consumer named
/// `correlation-index--<kind>--<context>--<uuid v7>`. The consumer is deleted when
/// the projector stops, fails or is aborted; readers report a source without
/// such a consumer as inactive.
pub struct NatsCorrelationIndexProjector {
    context: jetstream::Context,
    stores: Arc<IndexStores>,
    event_store: NatsEventStoreConfig,
    integration_stream: Option<String>,
}

impl NatsCorrelationIndexProjector {
    /// Verifies the bucket and both source streams; never provisions them.
    ///
    /// Fails with `ConfigurationMismatch` when a source stream uses work-queue
    /// or interest retention, or limits its consumers: the projector's
    /// unacknowledged consumer would remove messages from the former and could
    /// exhaust the latter.
    pub async fn connect(
        context: jetstream::Context,
        index: &NatsReadModelConfig,
        event_store: NatsEventStoreConfig,
        topology: &MessagingTopology,
    ) -> Result<Self, ReadModelError> {
        if topology.application() != event_store.application()
            || topology.traffic_scope() != event_store.traffic_scope()
        {
            return Err(foreign_source());
        }
        let integration_stream = topology.integration_event_stream().as_str().to_owned();
        Self::connect_sources(context, index, event_store, Some(integration_stream)).await
    }

    /// Projects only the domain events of a context that publishes no
    /// integration events, for example one without an integration stream.
    ///
    /// Fails with `ConfigurationMismatch` when the index already holds the
    /// context's integration source: that source would stay registered without
    /// a projector. Delete and rebuild the index to drop it.
    pub async fn connect_domain_only(
        context: jetstream::Context,
        index: &NatsReadModelConfig,
        event_store: NatsEventStoreConfig,
    ) -> Result<Self, ReadModelError> {
        let projector = Self::connect_sources(context, index, event_store, None).await?;
        let integration = source_key(
            CorrelationSourceKind::Integration,
            projector.event_store.bounded_context().as_str(),
        )?;
        if projector.stores.source(&integration).await?.is_some() {
            return Err(ReadModelError::new(
                ReadModelErrorKind::ConfigurationMismatch,
                "the correlation index already projects this context's integration events; \
                 delete and rebuild the index to project its domain events only",
            ));
        }
        Ok(projector)
    }

    async fn connect_sources(
        context: jetstream::Context,
        index: &NatsReadModelConfig,
        event_store: NatsEventStoreConfig,
        integration_stream: Option<String>,
    ) -> Result<Self, ReadModelError> {
        if index.application() != event_store.application()
            || index.traffic_scope() != event_store.traffic_scope()
        {
            return Err(foreign_source());
        }
        let stores = IndexStores::connect(&context, index).await?;
        for stream in
            std::iter::once(event_store.stream_name()).chain(integration_stream.as_deref())
        {
            verify_source_stream(&context, stream).await?;
        }
        Ok(Self {
            context,
            stores: Arc::new(stores),
            event_store,
            integration_stream,
        })
    }

    /// Runs until shutdown. Transient unavailability is retried from the stored
    /// checkpoint; capacity, policy and corrupt-history failures stop the
    /// projector and are returned, without evicting existing entries.
    pub async fn run_until_shutdown(
        self,
        shutdown: watch::Receiver<bool>,
    ) -> Result<(), ReadModelError> {
        if *shutdown.borrow() {
            return Ok(());
        }
        let context_name = self.event_store.bounded_context().as_str().to_owned();
        let integration_filter = format!(
            "{}.integration.{context_name}.>",
            traffic_subject_prefix(
                self.event_store.application().as_str(),
                self.event_store.traffic_scope()
            )
        );
        let domain = SourceLoop {
            kind: CorrelationSourceKind::Domain,
            context_name: context_name.clone(),
            stream: self.event_store.stream_name().to_owned(),
            filter_subject: self.event_store.aggregate_subject_filter(),
        };
        let Some(integration_stream) = self.integration_stream.clone() else {
            return Box::pin(domain.run(&self, shutdown)).await;
        };
        let integration = SourceLoop {
            kind: CorrelationSourceKind::Integration,
            context_name,
            stream: integration_stream,
            filter_subject: integration_filter,
        };
        tokio::try_join!(
            Box::pin(domain.run(&self, shutdown.clone())),
            Box::pin(integration.run(&self, shutdown)),
        )
        .map(|_| ())
    }
}

fn stream_range(stream: &jetstream::stream::Stream) -> StreamRange {
    let state = &stream.cached_info().state;
    StreamRange {
        first_sequence: state.first_sequence,
        last_sequence: state.last_sequence,
    }
}

struct SourceLoop {
    kind: CorrelationSourceKind,
    context_name: String,
    stream: String,
    filter_subject: String,
}

impl SourceLoop {
    async fn run(
        &self,
        projector: &NatsCorrelationIndexProjector,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), ReadModelError> {
        loop {
            let result = self.project(projector, &mut shutdown).await;
            match result {
                Ok(()) => return Ok(()),
                Err(error) if error.kind() == ReadModelErrorKind::Unavailable => {
                    tracing::warn!(source = self.kind.as_str(), context = %self.context_name,
                        %error, "correlation index projection will be retried");
                }
                Err(error) => {
                    tracing::error!(source = self.kind.as_str(), context = %self.context_name,
                        %error, "correlation index projection stopped");
                    return Err(error);
                }
            }
            tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                }
                () = tokio::time::sleep(RETRY_DELAY) => {}
            }
        }
    }

    async fn project(
        &self,
        projector: &NatsCorrelationIndexProjector,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ReadModelError> {
        let stream = projector
            .context
            .get_stream(&self.stream)
            .await
            .map_err(|_| unavailable("correlation index source stream is unavailable"))?;
        let checkpoint = projector
            .stores
            .register(
                &IndexSource {
                    context: self.context_name.clone(),
                    kind: self.kind,
                    stream: self.stream.clone(),
                    filter_subject: self.filter_subject.clone(),
                    generation: None,
                    checkpoint: 0,
                    skipped: 0,
                },
                stream_range(&stream),
            )
            .await?;
        let start_sequence = checkpoint
            .checked_add(1)
            .ok_or_else(|| invalid_data("source checkpoint overflowed"))?;
        let name = projector_consumer_name(self.kind, &self.context_name);
        let lease = ConsumerLease {
            context: projector.context.clone(),
            stream: self.stream.clone(),
            name: Some(name.clone()),
        };
        let result = self
            .consume(projector, &stream, &name, start_sequence, shutdown)
            .await;
        lease.release().await;
        result
    }

    async fn consume(
        &self,
        projector: &NatsCorrelationIndexProjector,
        stream: &jetstream::stream::Stream,
        name: &str,
        start_sequence: u64,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), ReadModelError> {
        let consumer: consumer::OrderedPullConsumer = stream
            .create_consumer(consumer::pull::OrderedConfig {
                name: Some(name.to_owned()),
                description: Some("rostfrei correlation index".to_owned()),
                filter_subject: self.filter_subject.clone(),
                deliver_policy: DeliverPolicy::ByStartSequence { start_sequence },
                ..Default::default()
            })
            .await
            .map_err(|_| unavailable("failed to start correlation index delivery"))?;
        let mut messages = consumer
            .messages()
            .await
            .map_err(|_| unavailable("failed to start correlation index delivery"))?;
        loop {
            let first = tokio::select! {
                biased;
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                    continue;
                }
                message = messages.next() => message,
            };
            let mut batch = vec![delivered(first)?];
            while batch.len() < MAX_BATCH_MESSAGES {
                match tokio::time::timeout(BATCH_LINGER, messages.next()).await {
                    Ok(message) => batch.push(delivered(message)?),
                    Err(_) => break,
                }
            }
            self.apply(projector, batch).await?;
        }
    }

    async fn apply(
        &self,
        projector: &NatsCorrelationIndexProjector,
        batch: Vec<jetstream::Message>,
    ) -> Result<(), ReadModelError> {
        let mut grouped: BTreeMap<String, (CorrelationId, Vec<CorrelationRef>)> = BTreeMap::new();
        let mut checkpoint = 0;
        for message in &batch {
            let info = message
                .info()
                .map_err(|_| unavailable("delivery metadata is unavailable"))?;
            checkpoint = checkpoint.max(info.stream_sequence);
            let indexed = match self.kind {
                CorrelationSourceKind::Domain => {
                    let occurred_at = timestamp(info.published.unix_timestamp_nanos())?;
                    domain_reference(projector, message, &self.context_name, occurred_at)?
                }
                CorrelationSourceKind::Integration => {
                    integration_reference(message, &self.context_name, info.stream_sequence)
                }
            };
            if let Some((correlation_id, reference)) = indexed {
                grouped
                    .entry(correlation_id.as_str().to_owned())
                    .or_insert_with(|| (correlation_id, Vec::new()))
                    .1
                    .push(reference);
            }
        }
        for (correlation_id, references) in grouped.values() {
            projector.stores.append(correlation_id, references).await?;
        }
        projector
            .stores
            .advance(self.kind, &self.context_name, checkpoint)
            .await
    }
}

/// Deletes a projector consumer when its source loop ends. An aborted loop drops
/// the lease without awaiting; the deletion is then spawned, and the server
/// removes a consumer left behind by a lost process after 30 s of inactivity.
struct ConsumerLease {
    context: jetstream::Context,
    stream: String,
    name: Option<String>,
}

impl ConsumerLease {
    async fn release(mut self) {
        if let Some(name) = self.name.take() {
            delete_projector_consumer(&self.context, &self.stream, &name).await;
        }
    }
}

impl Drop for ConsumerLease {
    fn drop(&mut self) {
        let Some(name) = self.name.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let context = self.context.clone();
            let stream = std::mem::take(&mut self.stream);
            runtime.spawn(async move {
                delete_projector_consumer(&context, &stream, &name).await;
            });
        }
    }
}

async fn delete_projector_consumer(context: &jetstream::Context, stream: &str, name: &str) {
    if let Err(error) = context.delete_consumer_from_stream(name, stream).await {
        let missing = matches!(error.kind(),
            jetstream::stream::ConsumerErrorKind::JetStream(error)
                if error.error_code() == jetstream::ErrorCode::CONSUMER_NOT_FOUND);
        if !missing {
            tracing::warn!(%stream, consumer = %name, %error,
                "correlation index consumer cleanup failed; the server removes it after inactivity");
        }
    }
}

fn projector_consumer_prefix(kind: CorrelationSourceKind, context: &str) -> String {
    format!(
        "{PROJECTOR_CONSUMER_PREFIX}--{}--{context}--",
        kind.as_str()
    )
}

fn projector_consumer_name(kind: CorrelationSourceKind, context: &str) -> String {
    format!(
        "{}{}",
        projector_consumer_prefix(kind, context),
        Uuid::now_v7()
    )
}

fn is_projector_consumer(name: &str, kind: CorrelationSourceKind, context: &str) -> bool {
    name.strip_prefix(&projector_consumer_prefix(kind, context))
        .is_some_and(|id| Uuid::try_parse(id).is_ok())
}

/// Ordered consumers acknowledge nothing. On a work-queue or interest stream
/// their deliveries would remove messages; a consumer limit could be exhausted
/// by the projector and the reader's lag cursors.
async fn verify_source_stream(
    context: &jetstream::Context,
    name: &str,
) -> Result<(), ReadModelError> {
    let stream = context
        .get_stream(name)
        .await
        .map_err(|_| unavailable("correlation index source stream is unavailable"))?;
    let config = &stream.cached_info().config;
    if matches!(
        config.retention,
        RetentionPolicy::WorkQueue | RetentionPolicy::Interest
    ) {
        return Err(ReadModelError::new(
            ReadModelErrorKind::ConfigurationMismatch,
            format!(
                "correlation index source stream `{name}` must use limits retention; \
                 work-queue and interest streams lose messages to the index consumer"
            ),
        ));
    }
    if config.max_consumers > 0 {
        return Err(ReadModelError::new(
            ReadModelErrorKind::ConfigurationMismatch,
            format!(
                "correlation index source stream `{name}` limits its consumers to {}; \
                 the index needs unlimited ephemeral consumers",
                config.max_consumers
            ),
        ));
    }
    Ok(())
}

fn delivered(
    message: Option<Result<jetstream::Message, consumer::pull::OrderedError>>,
) -> Result<jetstream::Message, ReadModelError> {
    match message {
        Some(Ok(message)) => Ok(message),
        Some(Err(_)) | None => Err(unavailable("correlation index delivery was interrupted")),
    }
}

fn domain_reference(
    projector: &NatsCorrelationIndexProjector,
    message: &jetstream::Message,
    context: &str,
    occurred_at: MessageTimestamp,
) -> Result<Option<(CorrelationId, CorrelationRef)>, ReadModelError> {
    let headers = message
        .headers
        .as_ref()
        .ok_or_else(|| invalid_data("stored domain event has no headers"))?;
    let decoded = decode_consumed_event(
        &projector.event_store,
        message.subject.as_str(),
        headers,
        &message.payload,
    )
    .map_err(|error| invalid_data(&format!("stored domain event is invalid: {error}")))?;
    Ok(decoded.recorded.correlation_id().map(|correlation_id| {
        (
            correlation_id.clone(),
            CorrelationRef::Domain(DomainEventRef::from_recorded(
                context,
                &decoded.recorded,
                occurred_at,
            )),
        )
    }))
}

fn integration_reference(
    message: &jetstream::Message,
    context: &str,
    sequence: u64,
) -> Option<(CorrelationId, CorrelationRef)> {
    let address = IntegrationEventAddress::parse(message.subject.as_str()).ok();
    let metadata = serde_json::from_slice::<IntegrationMetadata>(&message.payload).ok();
    let (Some(address), Some(metadata)) = (address, metadata) else {
        tracing::warn!(subject = %message.subject, sequence,
            "skipping an integration message without a valid envelope");
        return None;
    };
    if address.context() != context {
        return None;
    }
    Some((
        metadata.correlation_id,
        CorrelationRef::Integration(IntegrationEventRef {
            context: context.to_owned(),
            subject: address.as_str().to_owned(),
            message_id: metadata.message_id.as_str().to_owned(),
            event_type: address.name().to_owned(),
            schema_version: metadata.schema_version.get(),
            causation_id: metadata.causation_id.map(|id| id.as_str().to_owned()),
            occurred_at: metadata.occurred_at,
            sequence,
        }),
    ))
}

/// Reads the application-scoped index. Sources and their streams are discovered
/// from the bucket itself; readers need no list of bounded contexts.
#[derive(Clone)]
pub struct NatsCorrelationIndexReader {
    context: jetstream::Context,
    stores: Arc<IndexStores>,
}

impl NatsCorrelationIndexReader {
    /// Verification-only: the bucket must already be provisioned.
    pub async fn connect(
        context: jetstream::Context,
        index: &NatsReadModelConfig,
    ) -> Result<Self, ReadModelError> {
        let stores = IndexStores::connect(&context, index).await?;
        Ok(Self {
            context,
            stores: Arc::new(stores),
        })
    }

    async fn projector_consumers(&self, stream: &str) -> Result<Vec<String>, ReadModelError> {
        let names = self
            .context
            .get_stream_no_info(stream)
            .await
            .map_err(|_| unavailable("correlation index source stream is unavailable"))?
            .consumer_names()
            .filter_map(|name| async move {
                match name {
                    Ok(name) if name.starts_with(PROJECTOR_CONSUMER_PREFIX) => Some(Ok(name)),
                    Ok(_) => None,
                    Err(_) => Some(Err(unavailable(
                        "failed to list correlation index consumers",
                    ))),
                }
            })
            .collect::<Vec<_>>()
            .await;
        names.into_iter().collect()
    }

    /// Unindexed matching messages and sequences the stream dropped after the
    /// checkpoint before a projector reached them.
    async fn pending(&self, source: &IndexSource) -> Result<(u64, u64), ReadModelError> {
        let stream = self
            .context
            .get_stream(&source.stream)
            .await
            .map_err(|_| unavailable("correlation index source stream is unavailable"))?;
        let gap = match SourcePosition::classify(source.checkpoint, stream_range(&stream)) {
            SourcePosition::Recreated => return Err(recreated()),
            SourcePosition::Gap(skipped) => skipped,
            SourcePosition::Consistent => 0,
        };
        let last = match stream
            .get_last_raw_message_by_subject(&source.filter_subject)
            .await
        {
            Ok(message) => message.sequence,
            Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => {
                return Ok((0, gap));
            }
            Err(_) => return Err(unavailable("failed to read source stream position")),
        };
        if last <= source.checkpoint {
            return Ok((0, gap));
        }
        let start_sequence = source
            .checkpoint
            .checked_add(1)
            .ok_or_else(|| invalid_data("source checkpoint overflowed"))?;
        let mut cursor = stream
            .create_consumer(consumer::pull::Config {
                filter_subject: source.filter_subject.clone(),
                deliver_policy: DeliverPolicy::ByStartSequence { start_sequence },
                ack_policy: AckPolicy::None,
                memory_storage: true,
                num_replicas: 1,
                inactive_threshold: LAG_CURSOR_INACTIVE_THRESHOLD,
                ..Default::default()
            })
            .await
            .map_err(|_| unavailable("failed to count unindexed source messages"))?;
        let name = cursor.cached_info().name.clone();
        let pending = cursor
            .info()
            .await
            .map(|info| info.num_pending)
            .map_err(|_| unavailable("failed to count unindexed source messages"));
        if self
            .context
            .delete_consumer_from_stream(&name, &source.stream)
            .await
            .is_err()
        {
            tracing::warn!(stream = %source.stream, consumer = %name,
                "lag cursor cleanup failed; the server removes it after inactivity");
        }
        pending.map(|pending| (pending, gap))
    }
}

#[async_trait]
impl CorrelationIndexReader for NatsCorrelationIndexReader {
    async fn chain(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<CorrelationChain, ReadModelError> {
        self.stores.chain(correlation_id).await
    }

    async fn freshness(&self) -> Result<CorrelationIndexFreshness, ReadModelError> {
        let Some(directory) = self
            .stores
            .directory
            .read(&ReadModelKey::new(SOURCES_KEY)?)
            .await?
        else {
            return Ok(CorrelationIndexFreshness::default());
        };
        let mut consumers: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sources = Vec::new();
        for key in directory.value.sources {
            let Some(source) = self.stores.source(&ReadModelKey::new(key)?).await? else {
                continue;
            };
            if !consumers.contains_key(&source.stream) {
                let names = self.projector_consumers(&source.stream).await?;
                consumers.insert(source.stream.clone(), names);
            }
            let active = consumers.get(&source.stream).is_some_and(|names| {
                names
                    .iter()
                    .any(|name| is_projector_consumer(name, source.kind, &source.context))
            });
            let (behind, gap) = self.pending(&source).await?;
            sources.push(CorrelationSourceFreshness {
                context: source.context,
                kind: source.kind,
                stream: source.stream,
                state: if active {
                    CorrelationSourceState::Active
                } else {
                    CorrelationSourceState::Inactive
                },
                behind,
                skipped: source.skipped.saturating_add(gap),
            });
        }
        Ok(CorrelationIndexFreshness::from_sources(sources))
    }

    async fn integration_available(
        &self,
        reference: &IntegrationEventRef,
    ) -> Result<bool, ReadModelError> {
        let key = source_key(CorrelationSourceKind::Integration, &reference.context)?;
        let Some(source) = self.stores.source(&key).await? else {
            return Ok(false);
        };
        let stream = self
            .context
            .get_stream(&source.stream)
            .await
            .map_err(|_| unavailable("integration-event stream is unavailable"))?;
        match stream.get_raw_message(reference.sequence).await {
            Ok(message) => Ok(message.subject.as_str() == reference.subject
                && message
                    .headers
                    .get("Nats-Msg-Id")
                    .is_some_and(|id| id.as_str() == reference.message_id)),
            Err(error) if error.kind() == RawMessageErrorKind::NoMessageFound => Ok(false),
            Err(_) => Err(unavailable("failed to read integration-event stream")),
        }
    }
}

async fn open<T>(
    context: &jetstream::Context,
    config: &NatsReadModelConfig,
) -> Result<Store<T>, ReadModelError>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    NatsReadModelStore::connect(
        context.clone(),
        config.clone(),
        JsonReadModelCodec::new(NonZeroU32::MIN),
    )
    .await
}

/// Bounded optimistic read-modify-write of one key. `change` returns `None`
/// when the stored value already satisfies the request.
async fn cas<T, F>(store: &Store<T>, key: &ReadModelKey, change: F) -> Result<T, ReadModelError>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
    F: Fn(Option<&T>) -> Result<Option<T>, ReadModelError> + Send + Sync,
{
    for _ in 0..MAX_WRITE_ATTEMPTS {
        let stored = store.read(key).await?;
        let Some(next) = change(stored.as_ref().map(|entry| &entry.value))? else {
            if let Some(stored) = stored {
                return Ok(stored.value);
            }
            return Err(invalid_data("correlation index update produced no value"));
        };
        let written = match &stored {
            Some(entry) => store.update(key, &entry.revision, &next).await,
            None => store.create(key, &next).await,
        };
        match written {
            Ok(_) => return Ok(next),
            Err(error) if error.kind() == ReadModelErrorKind::Conflict => {}
            Err(error) => return Err(error),
        }
    }
    Err(contention())
}

fn correlation_key(correlation_id: &CorrelationId) -> String {
    let value = correlation_id.as_str();
    let literal = value.len() <= MAX_LITERAL_KEY_BYTES
        && !value.split('.').any(str::is_empty)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'=' | b'.'));
    if literal {
        format!("{LITERAL_KEY_PREFIX}{value}")
    } else {
        let digest = crate::hex::encode_lower_hex(Sha256::digest(value.as_bytes()));
        format!("{DIGEST_KEY_PREFIX}{digest}")
    }
}

fn page_key(base: &str, page: usize) -> Result<ReadModelKey, ReadModelError> {
    if page == 0 {
        ReadModelKey::new(base)
    } else {
        ReadModelKey::new(format!("{base}/{page}"))
    }
}

fn source_key(kind: CorrelationSourceKind, context: &str) -> Result<ReadModelKey, ReadModelError> {
    ReadModelKey::new(format!("source.{}.{context}", kind.as_str()))
}

fn timestamp(nanos: i128) -> Result<MessageTimestamp, ReadModelError> {
    nanos
        .checked_div(1_000_000)
        .and_then(|millis| u64::try_from(millis).ok())
        .and_then(|millis| MessageTimestamp::from_unix_milliseconds(millis).ok())
        .ok_or_else(|| invalid_data("stored event time is out of range"))
}

fn foreign_source() -> ReadModelError {
    ReadModelError::new(
        ReadModelErrorKind::ConfigurationMismatch,
        "correlation index sources must belong to its application and traffic scope",
    )
}

fn unavailable(message: &str) -> ReadModelError {
    ReadModelError::new(ReadModelErrorKind::Unavailable, message)
}

fn invalid_data(message: &str) -> ReadModelError {
    ReadModelError::new(ReadModelErrorKind::InvalidData, message)
}

fn contention() -> ReadModelError {
    unavailable("correlation index write contention exceeded its retry budget")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn correlation(value: &str) -> CorrelationId {
        CorrelationId::new(value).unwrap()
    }

    #[test]
    fn readable_correlations_keep_their_identity_and_others_use_a_digest() {
        assert_eq!(
            correlation_key(&correlation("op-1.retry_2")),
            "correlation.op-1.retry_2"
        );
        for value in ["a/b", "a:b", "a..b", ".a", "a*", &"x".repeat(241)] {
            let key = correlation_key(&correlation(value));
            assert!(key.starts_with(DIGEST_KEY_PREFIX), "{value}");
            assert_eq!(key.len(), DIGEST_KEY_PREFIX.len() + 64);
        }
        let longest = correlation_key(&correlation(&"x".repeat(MAX_LITERAL_KEY_BYTES)));
        page_key(&longest, MAX_CORRELATION_PAGES - 1).unwrap();
    }

    #[test]
    fn projector_consumers_are_recognized_per_kind_and_context() {
        let name = projector_consumer_name(CorrelationSourceKind::Domain, "ordering");
        assert!(name.starts_with("correlation-index--domain--ordering--"));
        assert!(is_projector_consumer(
            &name,
            CorrelationSourceKind::Domain,
            "ordering"
        ));
        assert!(!is_projector_consumer(
            &name,
            CorrelationSourceKind::Integration,
            "ordering"
        ));
        assert!(!is_projector_consumer(
            &name,
            CorrelationSourceKind::Domain,
            "order"
        ));
        let nested = projector_consumer_name(CorrelationSourceKind::Domain, "ordering--eu");
        assert!(!is_projector_consumer(
            &nested,
            CorrelationSourceKind::Domain,
            "ordering"
        ));
        assert!(!is_projector_consumer(
            "correlation-index--domain--ordering--durable",
            CorrelationSourceKind::Domain,
            "ordering"
        ));
    }

    fn range(first_sequence: u64, last_sequence: u64) -> StreamRange {
        StreamRange {
            first_sequence,
            last_sequence,
        }
    }

    #[test]
    fn sequence_regression_is_a_recreation_and_lost_retention_a_gap() {
        use SourcePosition::{Consistent, Gap, Recreated};
        for (checkpoint, first, last, expected) in [
            (0, 0, 0, Consistent),
            (0, 5, 9, Consistent),
            (3, 1, 9, Consistent),
            (3, 4, 9, Consistent),
            (9, 10, 9, Consistent),
            (3, 7, 9, Gap(3)),
            (3, 10, 9, Gap(6)),
            (3, 0, 0, Recreated),
            (5, 1, 2, Recreated),
        ] {
            assert_eq!(
                SourcePosition::classify(checkpoint, range(first, last)),
                expected,
                "checkpoint {checkpoint}, retained {first}..={last}"
            );
        }
    }

    #[test]
    fn source_records_with_a_created_time_generation_still_decode() {
        let stored: IndexSource = serde_json::from_value(serde_json::json!({
            "context": "ordering", "kind": "integration", "stream": "SHOP_INTEGRATION_EVENTS",
            "filter_subject": "shop.integration.ordering.>",
            "generation": "1792956416874289669", "checkpoint": 7,
        }))
        .unwrap();
        assert_eq!(stored.checkpoint, 7);
        assert_eq!(stored.skipped, 0);
        let encoded = serde_json::to_value(&stored).unwrap();
        assert!(encoded.get("generation").is_none());
        assert!(encoded.get("skipped").is_none());
    }

    #[test]
    fn index_keys_cannot_collide_with_source_progress() {
        let reserved = correlation_key(&correlation("sources"));
        assert_ne!(reserved, SOURCES_KEY);
        assert!(!reserved.starts_with("source."));
    }
}
