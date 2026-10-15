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
}

impl AggregateCodecs {
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
