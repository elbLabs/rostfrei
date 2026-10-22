use async_nats::jetstream::{self, message::StreamMessage, stream::LastRawMessageErrorKind};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rostfrei_core::ContentFingerprint;
use rostfrei_messaging_core::{
    CommandAddress, CommandEnvelope, CommandResponse, CommandResponseOutcome,
    IntegrationEventAddress, IntegrationEventEnvelope, TrafficScope,
    derive_command_response_address,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{CurrentBackend, NatsRuntime, NatsRuntimeConfig};
use crate::{
    CatalogHistorical, HistoricalError as Error, HistoricalMessage, HistoricalOrder,
    HistoricalPage, HistoricalQuery, HistoricalReader, ObservationScope, TracePayloadPolicy,
};

const MAX_SCAN: usize = 500;
mod snapshots;
const MAX_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct NatsHistoricalReader {
    codecs: Arc<rostfrei_core::AggregateCodecs>,
    context: jetstream::Context,
    config: NatsRuntimeConfig,
    backend: CurrentBackend,
    gate: Arc<RwLock<()>>,
    catalog: CatalogHistorical,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Filter {
    aggregate_type: Option<String>,
    aggregate_id: Option<String>,
    correlation_id: Option<String>,
}

impl Filter {
    fn matches(&self, message: &HistoricalMessage) -> bool {
        self.aggregate_type
            .as_ref()
            .is_none_or(|value| message.aggregate_type.as_ref() == Some(value))
            && self
                .correlation_id
                .as_ref()
                .is_none_or(|value| message.correlation_id.as_ref() == Some(value))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Position {
    generation: String,
    next: u64,
    last: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    binding: String,
    filter: Filter,
    limit: usize,
    source: usize,
    #[serde(default)]
    order: HistoricalOrder,
    positions: Vec<Position>,
}

impl Cursor {
    fn from_query(query: HistoricalQuery, binding: String) -> Result<Self, Error> {
        if let Some(encoded) = query.cursor {
            if encoded.len() > 8192
                || query.aggregate_type.is_some()
                || query.aggregate_id.is_some()
                || query.correlation_id.is_some()
                || query.limit.is_some()
                || query.order.is_some()
            {
                return Err(Error::InvalidQuery);
            }
            let bytes = URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| Error::InvalidQuery)?;
            let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidQuery)?;
            if cursor.version != 1 || cursor.binding != binding || cursor.positions.is_empty() {
                return Err(Error::InvalidQuery);
            }
            Ok(cursor)
        } else {
            Ok(Self {
                version: 1,
                binding,
                filter: Filter {
                    aggregate_type: query.aggregate_type,
                    aggregate_id: query.aggregate_id,
                    correlation_id: query.correlation_id,
                },
                limit: query.limit.unwrap_or(50),
                source: 0,
                order: query.order.unwrap_or_default(),
                positions: Vec::new(),
            })
        }
    }

    fn bind(&mut self, sources: &[Source]) -> Result<(), Error> {
        if self.positions.is_empty() {
            self.positions = sources
                .iter()
                .map(|source| Position {
                    generation: source.generation.clone(),
                    next: source.stream.cached_info().state.first_sequence.max(1),
                    last: source.stream.cached_info().state.last_sequence,
                })
                .collect();
        }
        if self.positions.len() != sources.len() || self.source >= sources.len() {
            return Err(Error::InvalidQuery);
        }
        for (position, source) in self.positions.iter().zip(sources) {
            if position.generation != source.generation {
                return Err(Error::Reset);
            }
            if position.next == 0
                || position.last > source.stream.cached_info().state.last_sequence
                || position.next > position.last.saturating_add(1)
            {
                return Err(Error::InvalidQuery);
            }
        }
        Ok(())
    }
}

struct Source {
    stream: jetstream::stream::Stream,
    filter: String,
    generation: String,
}

impl NatsHistoricalReader {
    pub fn new(runtime: &NatsRuntime) -> Self {
        let scope = match runtime.config.context.traffic_scope() {
            TrafficScope::Test => ObservationScope::Test,
            TrafficScope::Normal => ObservationScope::Production,
        };
        Self {
            codecs: runtime.bindings.codecs.clone(),
            context: runtime.connection.jetstream().clone(),
            config: runtime.config.clone(),
            backend: runtime.backend.clone(),
            gate: runtime.gate.clone(),
            catalog: CatalogHistorical {
                snapshot_aggregate_types: runtime.bindings.codecs.snapshot_aggregate_types(),
                aggregate_href: runtime
                    .bindings
                    .codecs
                    .snapshotting_enabled()
                    .then(|| format!("/historical/{}/aggregate", scope.as_str())),
                snapshot_fixture_href: runtime
                    .bindings
                    .codecs
                    .snapshotting_enabled()
                    .then(|| format!("/historical/{}/snapshot-fixture", scope.as_str())),
                application: runtime.config.context.application().as_str().to_owned(),
                scope,
                messages_href: format!("/historical/{}/messages", scope.as_str()),
                timeline_href: Some(format!(
                    "/historical/{}/messages?order=time",
                    scope.as_str()
                )),
                aggregate_types: runtime
                    .bindings
                    .metadata
                    .registry
                    .aggregates()
                    .map(str::to_owned)
                    .collect(),
            },
        }
    }

    fn validate(&self, filter: &Filter, limit: usize) -> Result<(), Error> {
        if !(1..=100).contains(&limit)
            || filter
                .aggregate_type
                .as_ref()
                .is_some_and(|value| !self.catalog.aggregate_types.contains(value))
            || filter.aggregate_id.is_some() && filter.aggregate_type.is_none()
        {
            return Err(Error::InvalidQuery);
        }
        if let Some(id) = &filter.aggregate_id {
            rostfrei_core::AggregateId::new(id.clone()).map_err(|_| Error::InvalidQuery)?;
        }
        if let Some(id) = &filter.correlation_id {
            rostfrei_messaging_core::CorrelationId::new(id.clone())
                .map_err(|_| Error::InvalidQuery)?;
        }
        Ok(())
    }

    async fn sources(&self, filter: &Filter) -> Result<Vec<Source>, Error> {
        let topology = self.config.messaging.topology();
        let mut plans = vec![(
            self.config.event_store.stream_name().to_owned(),
            self.config.event_store.stream_config().subjects,
            match (&filter.aggregate_type, &filter.aggregate_id) {
                (Some(kind), Some(id)) => self.config.event_store.aggregate_subject(kind, id),
                _ => self.config.event_store.aggregate_subject_filter(),
            },
        )];
        if filter.aggregate_type.is_none() {
            for config in [
                self.config.messaging.command_responses(),
                self.config.messaging.integration_events(),
                self.config.messaging.commands(),
            ] {
                let subjects: Vec<String> = config
                    .subjects()
                    .iter()
                    .map(|subject| subject.as_str().to_owned())
                    .collect();
                let subject = subjects.first().ok_or(Error::Unavailable)?.clone();
                plans.push((config.name().as_str().to_owned(), subjects, subject));
            }
        }
        let mut sources = Vec::new();
        for (name, subjects, filter) in plans {
            let stream = self
                .context
                .get_stream(&name)
                .await
                .map_err(|_| Error::Unavailable)?;
            let info = stream.cached_info();
            if info.config.subjects != subjects
                || info.config.mirror.is_some()
                || info.config.sources.is_some()
                || info.config.subject_transform.is_some()
            {
                return Err(Error::Unavailable);
            }
            let generation = ContentFingerprint::digest(format!(
                "{}\0{}\0{}\0{}",
                topology.application(),
                self.catalog.scope.as_str(),
                name,
                info.created
            ))
            .to_hex();
            sources.push(Source {
                stream,
                filter,
                generation,
            });
        }
        Ok(sources)
    }

    async fn read_inner(
        &self,
        query: HistoricalQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<HistoricalPage, Error> {
        // Pin every source while the runtime's reset gate is held; recheck broker generations
        // as well, so external stream recreation cannot silently join two histories.
        let _scope = self.gate.read().await;
        let store = self.backend.snapshot().store.clone();
        let binding = ContentFingerprint::digest(
            serde_json::to_vec(&self.catalog).map_err(|_| Error::Unavailable)?,
        )
        .to_hex();
        let mut cursor = Cursor::from_query(query, binding)?;
        self.validate(&cursor.filter, cursor.limit)?;
        let sources = self.sources(&cursor.filter).await?;
        cursor.bind(&sources)?;
        let mut items = Vec::new();
        let mut scanned = 0_usize;
        let mut skipped = 0_usize;
        let mut bytes = 0_usize;
        // Keep at most one raw head per stream. Only consumed positions enter the
        // cursor: lookahead is reread, never skipped, on the next bounded page.
        let mut heads: Vec<Option<StreamMessage>> = sources.iter().map(|_| None).collect();
        while cursor.source < sources.len()
            && items.len() < cursor.limit
            && scanned < MAX_SCAN
            && bytes < MAX_BYTES
        {
            let Some((index, stored)) =
                next_message(&sources, &mut cursor, &mut heads, &mut bytes).await?
            else {
                break;
            };
            let source = sources.get(index).ok_or(Error::InvalidQuery)?;
            let position = cursor.positions.get_mut(index).ok_or(Error::InvalidQuery)?;
            position.next = stored.sequence.checked_add(1).ok_or(Error::Unavailable)?;
            scanned = scanned.saturating_add(1);
            let Ok(mut message) = decode(&store, index, &stored, policy) else {
                skipped = skipped.saturating_add(1);
                continue;
            };
            if !cursor.filter.matches(&message) {
                continue;
            }
            message.id = format!("{}.{}", source.generation, stored.sequence);
            bytes = bytes.saturating_add(
                serde_json::to_vec(&message)
                    .map_err(|_| Error::Unavailable)?
                    .len(),
            );
            items.push(message);
        }
        self.check_generations(&sources).await?;
        // Avoid an empty trailing page when the last record exactly fills the page.
        while cursor
            .positions
            .get(cursor.source)
            .is_some_and(|position| position.next > position.last)
        {
            cursor.source = cursor.source.saturating_add(1);
        }
        let next_href = if cursor.source < sources.len() {
            Some(format!(
                "{}?cursor={}",
                self.catalog.messages_href,
                URL_SAFE_NO_PAD
                    .encode(serde_json::to_vec(&cursor).map_err(|_| Error::Unavailable)?)
            ))
        } else {
            None
        };
        Ok(HistoricalPage {
            generation: sources
                .first()
                .ok_or(Error::Unavailable)?
                .generation
                .clone(),
            application: self.catalog.application.clone(),
            scope: self.catalog.scope,
            order: cursor.order,
            items,
            scanned_messages: scanned,
            skipped_messages: skipped,
            next_href,
            partial: true,
        })
    }

    async fn check_generations(&self, sources: &[Source]) -> Result<(), Error> {
        for source in sources {
            let current = self
                .context
                .get_stream(&source.stream.cached_info().config.name)
                .await
                .map_err(|_| Error::Reset)?;
            if current.cached_info().created != source.stream.cached_info().created {
                return Err(Error::Reset);
            }
        }
        Ok(())
    }
}

async fn next_message(
    sources: &[Source],
    cursor: &mut Cursor,
    heads: &mut [Option<StreamMessage>],
    bytes: &mut usize,
) -> Result<Option<(usize, StreamMessage)>, Error> {
    for (index, source) in sources.iter().enumerate().skip(cursor.source) {
        let position = cursor.positions.get_mut(index).ok_or(Error::InvalidQuery)?;
        let head = heads.get_mut(index).ok_or(Error::InvalidQuery)?;
        if position.next > position.last {
            continue;
        }
        if head.is_none() {
            match source
                .stream
                .get_first_raw_message_by_subject(&source.filter, position.next)
                .await
            {
                Ok(stored) if stored.sequence <= position.last => {
                    if stored.sequence < position.next {
                        return Err(Error::Unavailable);
                    }
                    *bytes = bytes.saturating_add(stored.payload.len());
                    *head = Some(stored);
                }
                Ok(_) => position.next = position.last.saturating_add(1),
                Err(error) if error.kind() == LastRawMessageErrorKind::NoMessageFound => {
                    position.next = position.last.saturating_add(1);
                }
                Err(_) => return Err(Error::Unavailable),
            }
        }
        if cursor.order == HistoricalOrder::Source && head.is_some() {
            cursor.source = index;
            return Ok(head.take().map(|stored| (index, stored)));
        }
    }
    // Stable tie-breaks preserve equal-timestamp evidence across one-record pages.
    let index = heads
        .iter()
        .enumerate()
        .filter_map(|(index, head)| head.as_ref().map(|stored| (index, stored)))
        .min_by_key(|(index, stored)| (stored.time, *index, stored.sequence))
        .map(|(index, _)| index);
    Ok(index.and_then(|index| heads.get_mut(index)?.take().map(|stored| (index, stored))))
}

#[async_trait]
impl HistoricalReader for NatsHistoricalReader {
    fn catalog(&self) -> CatalogHistorical {
        self.catalog.clone()
    }
    async fn read(
        &self,
        query: HistoricalQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<HistoricalPage, Error> {
        self.read_inner(query, policy).await
    }
    async fn aggregate(
        &self,
        query: crate::HistoricalAggregateQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<crate::HistoricalAggregate, Error> {
        self.read_aggregate(query, policy).await
    }
    async fn snapshot_fixture(
        &self,
        query: crate::HistoricalAggregateQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<crate::Fixture, Error> {
        self.export_snapshot_fixture(query, policy).await
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "four distinct persisted wire formats share one inspection record"
)]
fn decode(
    store: &rostfrei_nats::NatsEventStore,
    source: usize,
    stored: &StreamMessage,
    policy: &dyn TracePayloadPolicy,
) -> Result<HistoricalMessage, Error> {
    let mut message = HistoricalMessage {
        id: String::new(),
        kind: "domain-event",
        evidence: "domain-event",
        message_id: String::new(),
        correlation_id: None,
        causation_id: None,
        name: String::new(),
        schema_version: None,
        context: store.config().bounded_context().as_str().to_owned(),
        aggregate_type: None,
        aggregate_id: None,
        stream_version: None,
        stored_at: stored.time.to_string(),
        stored_at_unix_nanos: stored.time.unix_timestamp_nanos().to_string(),
        payload: None,
        response: None,
    };
    match source {
        0 => {
            let event = store
                .decode_observed_event(stored.subject.as_str(), &stored.headers, &stored.payload)
                .map_err(|_| Error::Unavailable)?;
            event
                .event_id()
                .as_str()
                .clone_into(&mut message.message_id);
            message.correlation_id = event.correlation_id().map(|id| id.as_str().to_owned());
            message.causation_id = event.causation_id().map(|id| id.as_str().to_owned());
            event.event_type().clone_into(&mut message.name);
            message.schema_version = Some(event.schema_version());
            message.aggregate_type = Some(event.stream_id().aggregate_type().as_str().to_owned());
            message.aggregate_id = Some(event.stream_id().aggregate_id().as_str().to_owned());
            message.stream_version = Some(event.stream_version().value().to_string());
            message.payload = serde_json::from_slice(event.payload())
                .ok()
                .and_then(|value| policy.observed_event_payload(value));
        }
        1 => {
            let response: CommandResponse =
                serde_json::from_slice(&stored.payload).map_err(|_| Error::Unavailable)?;
            let address = response.command_address();
            if address.application() != store.config().application().as_str()
                || address.traffic_scope() != store.config().traffic_scope()
                || derive_command_response_address(
                    address,
                    response.operation_id(),
                    response.command_message_id(),
                )
                .map_err(|_| Error::Unavailable)?
                .as_str()
                    != stored.subject.as_str()
            {
                return Err(Error::Unavailable);
            }
            message.kind = "command";
            message.evidence = "command-response";
            response
                .command_message_id()
                .as_str()
                .clone_into(&mut message.message_id);
            message.correlation_id = Some(response.correlation_id().as_str().to_owned());
            address.context().clone_into(&mut message.context);
            address.name().clone_into(&mut message.name);
            message.response = Some(match response.outcome() {
                CommandResponseOutcome::Accepted => json!({"status":"accepted"}),
                CommandResponseOutcome::Rejected(rejection) => {
                    json!({"status":"rejected", "rejection": policy.observed_rejection(rejection.clone()).map_err(|_| Error::Unavailable)?})
                }
                CommandResponseOutcome::ExecutionFailed(_) => {
                    json!({"status":"indeterminate", "message":"Retained technical diagnostic; completion is not established."})
                }
            });
        }
        2 => {
            let address: IntegrationEventAddress = stored
                .subject
                .as_str()
                .parse()
                .map_err(|_| Error::Unavailable)?;
            let envelope: IntegrationEventEnvelope<Value> =
                serde_json::from_slice(&stored.payload).map_err(|_| Error::Unavailable)?;
            if address.application() != store.config().application().as_str()
                || address.traffic_scope() != store.config().traffic_scope()
            {
                return Err(Error::Unavailable);
            }
            message.kind = "integration-event";
            message.evidence = "integration-event";
            envelope
                .message_id()
                .as_str()
                .clone_into(&mut message.message_id);
            message.correlation_id = Some(envelope.correlation_id().as_str().to_owned());
            message.causation_id = envelope.causation_id().map(|id| id.as_str().to_owned());
            address.name().clone_into(&mut message.name);
            address.context().clone_into(&mut message.context);
            message.schema_version = Some(envelope.schema_version().get());
            message.payload = policy.observed_event_payload(envelope.into_payload());
        }
        3 => {
            let address: CommandAddress = stored
                .subject
                .as_str()
                .parse()
                .map_err(|_| Error::Unavailable)?;
            let envelope: CommandEnvelope<Value> =
                serde_json::from_slice(&stored.payload).map_err(|_| Error::Unavailable)?;
            if address.application() != store.config().application().as_str()
                || address.traffic_scope() != store.config().traffic_scope()
            {
                return Err(Error::Unavailable);
            }
            message.kind = "command";
            message.evidence = "command";
            envelope
                .message_id()
                .as_str()
                .clone_into(&mut message.message_id);
            message.correlation_id = Some(envelope.correlation_id().as_str().to_owned());
            message.causation_id = envelope.causation_id().map(|id| id.as_str().to_owned());
            address.name().clone_into(&mut message.name);
            address.context().clone_into(&mut message.context);
            message.schema_version = Some(envelope.schema_version().get());
            message.payload = policy.observed_event_payload(envelope.into_payload());
        }
        _ => return Err(Error::InvalidQuery),
    }
    Ok(message)
}
