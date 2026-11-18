use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use rostfrei_core::{Aggregate, AggregateId, AggregateType, EventCodec, EventHistory, StreamId};
use serde::Serialize;

use super::{
    AccessScope, InsightRecord, InsightsError, InsightsSource, MAX_RECORDS, MAX_RESULT_BYTES,
    RecordRef, ResourceKind, Selection,
};

/// Explicit JSON event-history adapter. Bind a tenant-isolated, validated
/// `EventHistory` handle; use a custom `InsightsSource` for non-JSON event codecs.
pub struct EventHistorySource {
    scope: AccessScope,
    history: Arc<dyn EventHistory>,
}

impl EventHistorySource {
    pub fn new(scope: AccessScope, history: Arc<dyn EventHistory>) -> Result<Self, InsightsError> {
        scope.validate()?;
        Ok(Self { scope, history })
    }
}

fn stream(selection: &Selection) -> Result<StreamId, InsightsError> {
    Ok(StreamId::new(
        AggregateType::new(
            selection
                .aggregate_type
                .clone()
                .ok_or(InsightsError::Invalid)?,
        )
        .map_err(|_| InsightsError::Invalid)?,
        AggregateId::new(
            selection
                .aggregate_id
                .clone()
                .ok_or(InsightsError::Invalid)?,
        )
        .map_err(|_| InsightsError::Invalid)?,
    ))
}

#[async_trait]
impl InsightsSource for EventHistorySource {
    async fn read(
        &self,
        scope: &AccessScope,
        selection: &Selection,
    ) -> Result<Vec<InsightRecord>, InsightsError> {
        if scope != &self.scope {
            return Err(InsightsError::Forbidden);
        }
        if selection.resource.kind != ResourceKind::Event {
            return Err(InsightsError::Invalid);
        }
        let stream = stream(selection)?;
        let events = self
            .history
            .load(&stream)
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        if events.iter().fold(0usize, |size, event| {
            size.saturating_add(event.payload().len())
        }) > MAX_RESULT_BYTES
        {
            return Err(InsightsError::Capacity);
        }
        let mut records = Vec::new();
        for event in events {
            if event.stream_id() != &stream {
                return Err(InsightsError::Unavailable);
            }
            if event.event_type() != selection.resource.type_name
                || event.schema_version() != selection.resource.schema_version
                || (!selection.ids.is_empty()
                    && !selection
                        .ids
                        .iter()
                        .any(|id| id == event.event_id().as_str()))
            {
                continue;
            }
            let value =
                serde_json::from_slice(event.payload()).map_err(|_| InsightsError::Unavailable)?;
            records.push(InsightRecord {
                reference: RecordRef {
                    id: event.event_id().as_str().into(),
                    version: rostfrei_core::ContentFingerprint::digest(event.payload()).to_hex(),
                },
                value,
            });
            if records.len() > MAX_RECORDS {
                return Err(InsightsError::Capacity);
            }
        }
        Ok(records)
    }
}

/// Rehydrates the actual application aggregate through its registered codec.
/// The explicit schema version classifies the serialized state representation.
pub struct AggregateStateSource<A: Aggregate> {
    scope: AccessScope,
    history: Arc<dyn EventHistory>,
    codec: Arc<dyn EventCodec<A>>,
    schema_version: u32,
    marker: PhantomData<fn() -> A>,
}

impl<A: Aggregate> AggregateStateSource<A> {
    pub fn new(
        scope: AccessScope,
        history: Arc<dyn EventHistory>,
        codec: Arc<dyn EventCodec<A>>,
        schema_version: u32,
    ) -> Result<Self, InsightsError> {
        scope.validate()?;
        if schema_version == 0 {
            return Err(InsightsError::Invalid);
        }
        Ok(Self {
            scope,
            history,
            codec,
            schema_version,
            marker: PhantomData,
        })
    }
}

#[async_trait]
impl<A> InsightsSource for AggregateStateSource<A>
where
    A: Aggregate + 'static,
    A::State: Serialize,
{
    async fn read(
        &self,
        scope: &AccessScope,
        selection: &Selection,
    ) -> Result<Vec<InsightRecord>, InsightsError> {
        if scope != &self.scope {
            return Err(InsightsError::Forbidden);
        }
        if selection.resource.kind != ResourceKind::Aggregate
            || selection.resource.type_name != A::aggregate_type()
            || selection.resource.schema_version != self.schema_version
        {
            return Err(InsightsError::Invalid);
        }
        let stream = stream(selection)?;
        if stream.aggregate_type().as_str() != A::aggregate_type() {
            return Err(InsightsError::Invalid);
        }
        let events = self
            .history
            .load(&stream)
            .await
            .map_err(|_| InsightsError::Unavailable)?;
        if events.is_empty() {
            return Ok(vec![]);
        }
        let mut state = A::initial(&stream);
        let mut bytes = 0usize;
        for event in &events {
            bytes = bytes.saturating_add(event.payload().len());
            if bytes > MAX_RESULT_BYTES {
                return Err(InsightsError::Capacity);
            }
            if event.stream_id() != &stream {
                return Err(InsightsError::Unavailable);
            }
            let decoded = self
                .codec
                .decode(event)
                .map_err(|_| InsightsError::Unavailable)?;
            A::apply(&mut state, &decoded);
        }
        let value = serde_json::to_value(state).map_err(|_| InsightsError::Unavailable)?;
        let last = events.last().ok_or(InsightsError::Unavailable)?;
        let id = stream.aggregate_id().as_str().to_owned();
        if !selection.ids.is_empty() && !selection.ids.contains(&id) {
            return Ok(vec![]);
        }
        Ok(vec![InsightRecord {
            reference: RecordRef {
                id,
                version: format!(
                    "{}:{}",
                    last.event_id().as_str(),
                    last.stream_version().value()
                ),
            },
            value,
        }])
    }
}
