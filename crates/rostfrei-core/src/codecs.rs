use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::Arc,
};

use thiserror::Error;

use crate::{
    Aggregate, AggregateType, Event, EventCodec, EventCodecError, EventCodecErrorKind,
    IdentityError, JsonEventCodec, RecordedEvent, StreamId,
};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CodecRegistrationError {
    #[error("invalid snapshot configuration: {0}")]
    Snapshot(String),
    #[error("shared codec configuration must be changed before it is installed")]
    SharedConfiguration,
    #[error(transparent)]
    InvalidAggregateType(#[from] IdentityError),
    #[error("an explicit codec is already registered for `{aggregate_type}`")]
    Duplicate { aggregate_type: AggregateType },
    #[error("aggregate wire identity `{aggregate_type}` belongs to a different Rust model")]
    ModelConflict { aggregate_type: AggregateType },
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CodecValidationError {
    #[error("no codec registered for `{aggregate_type}`")]
    Unregistered { aggregate_type: AggregateType },
    #[error("event `{event_type}` schema {schema_version} in `{stream_id}`: {source}")]
    Event {
        stream_id: StreamId,
        event_type: String,
        schema_version: u32,
        source: EventCodecError,
    },
}

trait HistoryValidator: Send + Sync {
    fn validate(
        &self,
        stream: &StreamId,
        events: &[RecordedEvent],
    ) -> Result<(), CodecValidationError>;
}

struct TypedValidator<A: Aggregate> {
    codec: Arc<dyn EventCodec<A>>,
    marker: PhantomData<fn() -> A>,
}

impl<A: Aggregate> HistoryValidator for TypedValidator<A> {
    fn validate(
        &self,
        stream: &StreamId,
        events: &[RecordedEvent],
    ) -> Result<(), CodecValidationError> {
        let mut state = A::initial(stream);
        for event in events {
            let decoded = if event.stream_id() == stream {
                self.codec.decode(event)
            } else {
                Err(EventCodecError::new(
                    EventCodecErrorKind::InvalidEnvelope,
                    "event belongs to another stream",
                ))
            }
            .map_err(|source| CodecValidationError::Event {
                stream_id: stream.clone(),
                event_type: event.event_type().to_owned(),
                schema_version: event.schema_version(),
                source,
            })?;
            A::apply(&mut state, &decoded);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Entry {
    model: TypeId,
    codec: Arc<dyn Any + Send + Sync>,
    validator: Arc<dyn HistoryValidator>,
}

/// One effective configuration. Typed resolution and erased fixture validation
/// share the exact same codec instance; the wire index is derived at registration.
#[derive(Clone, Default)]
pub struct AggregateCodecs {
    entries: HashMap<AggregateType, Entry>,
    pub(crate) snapshots: HashMap<AggregateType, crate::aggregate_loading::SnapshotEntry>,
    pub(crate) snapshot_store: Option<Arc<dyn crate::SnapshotStore>>,
    pub(crate) diagnostics: Option<Arc<crate::RebuildDiagnostics>>,
    pub(crate) snapshot_requests:
        Arc<std::sync::Mutex<std::collections::VecDeque<crate::SnapshotRequest>>>,
}

impl AggregateCodecs {
    pub fn snapshot_aggregate_types(&self) -> Vec<String> {
        if self.snapshot_store.is_none() {
            return Vec::new();
        }
        let mut types = self
            .snapshots
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        types.sort();
        types
    }

    pub async fn stored_snapshots(
        &self,
        history: &dyn crate::EventHistory,
        stream: &StreamId,
    ) -> Result<Vec<crate::StoredSnapshot>, crate::EventStoreError> {
        let entry = self
            .snapshots
            .get(stream.aggregate_type())
            .ok_or_else(|| crate::replay::invalid("snapshot codec is not configured"))?;
        let store = self
            .snapshot_store
            .as_ref()
            .ok_or_else(|| crate::replay::invalid("snapshot storage is not configured"))?;
        let authority = history
            .history_authority()
            .ok_or_else(|| crate::replay::invalid("history has no snapshot authority"))?;
        store
            .list(&crate::SnapshotKey::new(
                authority,
                stream,
                &entry.revision,
                entry.schema,
            ))
            .await
    }

    /// Check envelope integrity, current authority/replay identity and typed decoding.
    /// Original-history equivalence is verified separately when exporting/installing fixtures.
    pub fn validate_stored_snapshot(
        &self,
        history: &dyn crate::EventHistory,
        stream: &StreamId,
        snapshot: &crate::StoredSnapshot,
    ) -> Result<(), crate::CommandExecutionError> {
        let entry = self
            .snapshots
            .get(stream.aggregate_type())
            .ok_or_else(|| crate::replay::invalid("snapshot codec is not configured"))?;
        let authority = history
            .history_authority()
            .ok_or_else(|| crate::replay::invalid("history has no snapshot authority"))?;
        snapshot.validate()?;
        snapshot.checkpoint.validate_for(authority, stream)?;
        if snapshot.replay_revision != entry.revision || snapshot.schema_version != entry.schema {
            return Err(crate::replay::invalid(
                "snapshot replay revision or schema is incompatible",
            )
            .into());
        }
        entry.validate_state(&snapshot.state)?;
        Ok(())
    }

    pub async fn reconstruct(
        &self,
        history: &dyn crate::EventHistory,
        stream: &StreamId,
        version: Option<crate::StreamVersion>,
        use_snapshots: bool,
    ) -> Result<crate::AggregateReconstruction, crate::CommandExecutionError> {
        let entry = self
            .snapshots
            .get(stream.aggregate_type())
            .ok_or_else(|| crate::replay::invalid("snapshot codec is not configured"))?;
        entry
            .materializer
            .reconstruct(history, self, stream, version, use_snapshots)
            .await
    }

    /// Verify a portable fixture checkpoint against its full original prefix.
    pub fn prepare_fixture_snapshot(
        &self,
        stream: &StreamId,
        events: &[RecordedEvent],
        supplied: Option<&crate::SnapshotState>,
    ) -> Result<crate::SnapshotState, crate::CommandExecutionError> {
        let entry = self
            .snapshots
            .get(stream.aggregate_type())
            .ok_or_else(|| crate::replay::invalid("fixture snapshot codec is not configured"))?;
        if self.snapshot_store.is_none() {
            return Err(
                crate::replay::invalid("fixture snapshot storage is not configured").into(),
            );
        }
        let data = entry.materializer.fixture_state(self, stream, events)?;
        let state = crate::SnapshotState {
            replay_revision: entry.revision.clone(),
            schema_version: entry.schema,
            data,
        };
        if supplied.is_some_and(|supplied| supplied != &state) {
            return Err(crate::replay::invalid(
                "fixture snapshot does not match its original events or replay revision",
            )
            .into());
        }
        Ok(state)
    }

    /// Install already verified fixture state under the new history's native anchor.
    /// Rechecks state through original-event replay before publishing the cache entry.
    pub async fn install_fixture_snapshot(
        &self,
        history: &dyn crate::EventHistory,
        stream: &StreamId,
        version: crate::StreamVersion,
        state: &crate::SnapshotState,
    ) -> Result<(), crate::CommandExecutionError> {
        let replay = history.load_replay(stream, None, Some(version)).await?;
        let verified = self.prepare_fixture_snapshot(stream, &replay.events, Some(state))?;
        let checkpoint = replay.checkpoint.ok_or_else(|| {
            crate::replay::invalid("fixture snapshot requires a positive committed version")
        })?;
        let key = crate::SnapshotKey::new(
            &checkpoint.authority,
            stream,
            &verified.replay_revision,
            verified.schema_version,
        );
        let snapshot = crate::StoredSnapshot::new(
            verified.replay_revision,
            verified.schema_version,
            checkpoint,
            verified.data,
        )?;
        self.snapshot_store
            .as_ref()
            .ok_or_else(|| crate::replay::invalid("snapshot storage is not configured"))?
            .save(&key, &snapshot)
            .await?;
        Ok(())
    }

    pub fn snapshotting_enabled(&self) -> bool {
        self.snapshot_store.is_some() && !self.snapshots.is_empty()
    }

    #[must_use]
    pub fn with_snapshot_store(mut self, store: Arc<dyn crate::SnapshotStore>) -> Self {
        self.snapshot_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_rebuild_diagnostics(mut self, diagnostics: Arc<crate::RebuildDiagnostics>) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    pub fn register_json_snapshot<A>(
        &mut self,
        replay_revision: &str,
        schema_version: u32,
        policy: crate::SnapshotPolicy,
    ) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
        A::State: serde::Serialize + serde::de::DeserializeOwned + Send + Sync,
    {
        self.register_snapshot::<A, _>(
            replay_revision,
            schema_version,
            crate::JsonSnapshotCodec::<A>::default(),
            policy,
        )
    }

    pub fn register_snapshot<A, C>(
        &mut self,
        replay_revision: &str,
        schema_version: u32,
        codec: C,
        policy: crate::SnapshotPolicy,
    ) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
        A::State: Send + Sync,
        C: crate::SnapshotCodec<A> + 'static,
    {
        let aggregate_type = AggregateType::new(A::aggregate_type())?;
        if replay_revision.is_empty() || replay_revision.len() > 128 || schema_version == 0 {
            return Err(CodecRegistrationError::Snapshot(
                "a bounded replay revision and positive schema version are required".to_owned(),
            ));
        }
        if self.snapshots.contains_key(&aggregate_type) {
            return Err(CodecRegistrationError::Duplicate { aggregate_type });
        }
        if self
            .entries
            .get(&aggregate_type)
            .is_some_and(|entry| entry.model != TypeId::of::<A>())
        {
            return Err(CodecRegistrationError::ModelConflict { aggregate_type });
        }
        if !self.entries.contains_key(&aggregate_type) {
            self.register_json::<A>()?;
        }
        self.snapshots.insert(
            aggregate_type,
            crate::aggregate_loading::SnapshotEntry::new::<A, C>(
                replay_revision,
                schema_version,
                codec,
                policy,
            ),
        );
        Ok(())
    }

    pub fn pending_snapshots(&self) -> Vec<crate::SnapshotRequest> {
        self.snapshot_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
    pub fn register<A, C>(&mut self, codec: C) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        self.insert::<A, C>(codec, false)
    }

    pub fn register_json<A>(&mut self) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        self.register::<A, _>(JsonEventCodec)
    }

    /// Explicit override (or initial registration), before sharing this configuration.
    /// Cannot replace a different model occupying the same wire identity.
    pub fn replace<A, C>(&mut self, codec: C) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        self.insert::<A, C>(codec, true)
    }

    fn insert<A, C>(&mut self, codec: C, replace: bool) -> Result<(), CodecRegistrationError>
    where
        A: Aggregate + 'static,
        C: EventCodec<A> + 'static,
    {
        let aggregate_type = AggregateType::new(A::aggregate_type())?;
        if self.snapshots.contains_key(&aggregate_type) {
            return Err(CodecRegistrationError::Snapshot(
                "register event codecs before snapshot codecs so replay semantics are explicit"
                    .to_owned(),
            ));
        }
        if let Some(entry) = self.entries.get(&aggregate_type) {
            if entry.model != TypeId::of::<A>() {
                return Err(CodecRegistrationError::ModelConflict { aggregate_type });
            }
            if !replace {
                return Err(CodecRegistrationError::Duplicate { aggregate_type });
            }
        }
        let codec: Arc<dyn EventCodec<A>> = Arc::new(codec);
        self.entries.insert(
            aggregate_type,
            Entry {
                model: TypeId::of::<A>(),
                codec: Arc::new(Arc::clone(&codec)),
                validator: Arc::new(TypedValidator::<A> {
                    codec,
                    marker: PhantomData,
                }),
            },
        );
        Ok(())
    }

    pub fn resolve<A>(&self) -> Result<Arc<dyn EventCodec<A>>, EventCodecError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        let aggregate_type = AggregateType::new(A::aggregate_type()).map_err(|e| {
            EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string())
        })?;
        self.entries.get(&aggregate_type).map_or_else(
            || {
                let codec: Arc<dyn EventCodec<A>> = Arc::new(JsonEventCodec);
                Ok(codec)
            },
            |entry| {
                entry
                    .codec
                    .downcast_ref::<Arc<dyn EventCodec<A>>>()
                    .cloned()
                    .ok_or_else(|| {
                        EventCodecError::new(
                            EventCodecErrorKind::InvalidEnvelope,
                            "aggregate wire identity belongs to a different Rust model",
                        )
                    })
            },
        )
    }

    pub fn aggregate_types(&self) -> impl Iterator<Item = &AggregateType> {
        self.entries.keys()
    }
    pub fn contains(&self, aggregate_type: &AggregateType) -> bool {
        self.entries.contains_key(aggregate_type)
    }

    /// Decodes original facts and applies them without rewriting or exposing partial state.
    pub fn validate_history(
        &self,
        stream: &StreamId,
        events: &[RecordedEvent],
    ) -> Result<(), CodecValidationError> {
        self.entries
            .get(stream.aggregate_type())
            .ok_or_else(|| CodecValidationError::Unregistered {
                aggregate_type: stream.aggregate_type().clone(),
            })?
            .validator
            .validate(stream, events)
    }
}
