use super::{Error, Filter, NatsHistoricalReader, Source};
use crate::{
    HistoricalAggregate, HistoricalAggregateQuery, HistoricalSnapshot, TracePayloadPolicy,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rostfrei_core::{EventHistory, StreamId, StreamVersion};
use rostfrei_fixtures::{Fixture, FixtureAggregate, FixtureDomainEvent, FixtureSnapshot};
use rostfrei_messaging_core::{CorrelationId, MessageId, MessageSeries, SchemaVersion};
use serde_json::{Value, json};

fn version(value: Option<&str>) -> Result<Option<StreamVersion>, Error> {
    value
        .map(|value| {
            let parsed = value.parse::<u64>().map_err(|_| Error::InvalidQuery)?;
            if parsed.to_string() != value {
                return Err(Error::InvalidQuery);
            }
            Ok(StreamVersion::new(parsed))
        })
        .transpose()
}

fn state_value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|_| json!({"encoding":"base64","data":STANDARD.encode(bytes)}))
}

impl NatsHistoricalReader {
    async fn aggregate_sources(
        &self,
        query: &HistoricalAggregateQuery,
    ) -> Result<(StreamId, Vec<Source>), Error> {
        if !self
            .catalog
            .snapshot_aggregate_types
            .contains(&query.aggregate_type)
        {
            return Err(Error::NotConfigured);
        }
        let filter = Filter {
            aggregate_type: Some(query.aggregate_type.clone()),
            aggregate_id: Some(query.aggregate_id.clone()),
            correlation_id: None,
        };
        self.validate(&filter, 1)?;
        let stream = StreamId::new(
            rostfrei_core::AggregateType::new(query.aggregate_type.clone())
                .map_err(|_| Error::InvalidQuery)?,
            rostfrei_core::AggregateId::new(query.aggregate_id.clone())
                .map_err(|_| Error::InvalidQuery)?,
        );
        let sources = self.sources(&filter).await?;
        if query.generation.as_ref().is_some_and(|generation| {
            sources
                .first()
                .is_none_or(|source| generation != &source.generation)
        }) {
            return Err(Error::Reset);
        }
        Ok((stream, sources))
    }

    pub(super) async fn read_aggregate(
        &self,
        query: HistoricalAggregateQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<HistoricalAggregate, Error> {
        if query.snapshot_version.is_some() || query.fixture_id.is_some() {
            return Err(Error::InvalidQuery);
        }
        let _scope = self.gate.read().await;
        let (stream, sources) = self.aggregate_sources(&query).await?;
        let store = self.backend.snapshot().store.clone();
        let loaded = self
            .codecs
            .reconstruct(
                &store,
                &stream,
                version(query.version.as_deref())?,
                query.use_snapshots.unwrap_or(true),
            )
            .await
            .map_err(map_rebuild_error)?;
        let stored = self
            .codecs
            .stored_snapshots(&store, &stream)
            .await
            .map_err(map_store_error)?;
        let snapshots = stored
            .into_iter()
            .map(|snapshot| {
                let valid = self
                    .codecs
                    .validate_stored_snapshot(&store, &stream, &snapshot)
                    .is_ok();
                HistoricalSnapshot {
                    stream_version: snapshot.version().value().to_string(),
                    replay_revision: snapshot.replay_revision,
                    schema_version: snapshot.schema_version,
                    state_bytes: snapshot.state.len(),
                    valid,
                    state: valid
                        .then(|| policy.aggregate_state(state_value(&snapshot.state)))
                        .flatten(),
                }
            })
            .collect();
        self.check_generations(&sources).await?;
        Ok(HistoricalAggregate {
            application: self.catalog.application.clone(),
            scope: self.catalog.scope,
            generation: sources
                .first()
                .ok_or(Error::Unavailable)?
                .generation
                .clone(),
            aggregate_type: query.aggregate_type,
            aggregate_id: query.aggregate_id,
            version: loaded.version.value().to_string(),
            snapshot_version: loaded.report.snapshot_version.to_string(),
            snapshot_status: loaded.report.snapshot_status,
            events_read: loaded.report.events_read,
            events_applied: loaded.report.events_applied,
            state: policy.aggregate_state(state_value(&loaded.state)),
            snapshots,
        })
    }

    pub(super) async fn export_snapshot_fixture(
        &self,
        query: HistoricalAggregateQuery,
        policy: &dyn TracePayloadPolicy,
    ) -> Result<Fixture, Error> {
        if query.use_snapshots.is_some() {
            return Err(Error::InvalidQuery);
        }
        let selected = version(query.snapshot_version.as_deref())?
            .filter(|v| v.value() > 0)
            .ok_or(Error::InvalidQuery)?;
        let target = version(query.version.as_deref())?.unwrap_or(selected);
        // Export is a complete portable artifact; never silently truncate backing history.
        if target < selected {
            return Err(Error::InvalidQuery);
        }
        if target.value() > 10_000 {
            return Err(Error::Capacity);
        }
        let id = query.fixture_id.as_deref().ok_or(Error::InvalidQuery)?;
        let _scope = self.gate.read().await;
        let (stream, sources) = self.aggregate_sources(&query).await?;
        let store = self.backend.snapshot().store.clone();
        let snapshot = self
            .codecs
            .stored_snapshots(&store, &stream)
            .await
            .map_err(map_store_error)?
            .into_iter()
            .find(|snapshot| snapshot.version() == selected)
            .ok_or(Error::InvalidQuery)?;
        self.codecs
            .validate_stored_snapshot(&store, &stream, &snapshot)
            .map_err(map_rebuild_error)?;
        let state = state_value(&snapshot.state);
        if policy.aggregate_state(state.clone()).as_ref() != Some(&state) {
            return Err(Error::PayloadUnavailable);
        }
        let replay = store
            .load_replay(&stream, None, Some(target))
            .await
            .map_err(map_store_error)?;
        let prefix = replay
            .events
            .iter()
            .take_while(|event| event.stream_version() <= selected)
            .cloned()
            .collect::<Vec<_>>();
        self.codecs
            .prepare_fixture_snapshot(&stream, &prefix, Some(&(&snapshot).into()))
            .map_err(map_rebuild_error)?;
        let fixture = Fixture::new(
            id,
            format!("snapshot-{}-through-{}", selected.value(), target.value()),
            fixture_messages(&replay.events, id, policy)?,
        )
        .and_then(|fixture| {
            fixture
                .with_snapshots(vec![FixtureSnapshot::from_snapshot(&snapshot).map_err(
                    |_| rostfrei_fixtures::FixtureValidationError::InvalidSnapshot,
                )?])
        })
        .map_err(|_| Error::InvalidQuery)?;
        if serde_json::to_vec(&fixture)
            .map_err(|_| Error::Unavailable)?
            .len()
            > 4 * 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        self.check_generations(&sources).await?;
        Ok(fixture)
    }
}

fn fixture_messages(
    events: &[rostfrei_core::RecordedEvent],
    id: &str,
    policy: &dyn TracePayloadPolicy,
) -> Result<MessageSeries<FixtureDomainEvent>, Error> {
    let ids = events
        .iter()
        .map(|event| event.event_id().as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut messages = Vec::new();
    let mut payload_bytes = 0_usize;
    for event in events {
        payload_bytes = payload_bytes.saturating_add(event.payload().len());
        if payload_bytes > 4 * 1024 * 1024 {
            return Err(Error::Capacity);
        }
        let payload: Value =
            serde_json::from_slice(event.payload()).map_err(|_| Error::Unavailable)?;
        if policy.observed_event_payload(payload.clone()).as_ref() != Some(&payload) {
            return Err(Error::PayloadUnavailable);
        }
        let correlation = event
            .correlation_id()
            .map_or_else(
                || CorrelationId::new(format!("fixture:{id}")),
                |id| CorrelationId::new(id.as_str()),
            )
            .map_err(|_| Error::InvalidQuery)?;
        let parent = event
            .causation_id()
            .filter(|parent| ids.contains(parent.as_str()))
            .map(|id| MessageId::new(id.as_str()))
            .transpose()
            .map_err(|_| Error::InvalidQuery)?;
        messages.push(FixtureDomainEvent::new(
            MessageId::new(event.event_id().as_str()).map_err(|_| Error::InvalidQuery)?,
            correlation,
            parent,
            event.event_type(),
            SchemaVersion::new(event.schema_version()).map_err(|_| Error::InvalidQuery)?,
            FixtureAggregate::new(
                event.stream_id().aggregate_type().as_str(),
                event.stream_id().aggregate_id().as_str(),
            )
            .map_err(|_| Error::InvalidQuery)?,
            event.stream_version().value(),
            payload,
        ));
    }
    MessageSeries::try_from_nodes(messages).map_err(|_| Error::Capacity)
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "map_err adapter consumes the storage error"
)]
fn map_store_error(error: rostfrei_core::EventStoreError) -> Error {
    match error.kind() {
        rostfrei_core::EventStoreErrorKind::InvalidRequest => Error::InvalidQuery,
        rostfrei_core::EventStoreErrorKind::CapacityExhausted => Error::Capacity,
        _ => Error::Unavailable,
    }
}
fn map_rebuild_error(error: rostfrei_core::CommandExecutionError) -> Error {
    match error {
        rostfrei_core::CommandExecutionError::Store(error) => map_store_error(error),
        _ => Error::Unavailable,
    }
}
