use std::{collections::HashMap, convert::Infallible, fmt};

use domain::DomainEvent;
use rostfrei_core::{
    Aggregate, Event, EventCodec, EventCodecError, EventCodecErrorKind, EventId, EventVariant,
    JsonEventCodec, NewEvent, RecordedEvent,
};
use serde::de::DeserializeOwned;

/// Invalid historical-reader configuration, detected before a codec can be registered.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EventCodecBuildError {
    DuplicateMapping {
        event_type: &'static str,
        schema_version: u32,
    },
    CurrentSchemaMapping {
        event_type: &'static str,
        schema_version: u32,
    },
    InvalidSchemaVersion {
        event_type: &'static str,
        schema_version: u32,
    },
    RenamedEvent {
        source: &'static str,
        target: &'static str,
    },
    ConflictingTarget {
        event_type: &'static str,
        first: u32,
        second: u32,
    },
}

impl fmt::Display for EventCodecBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMapping {
                event_type,
                schema_version,
            } => write!(
                f,
                "duplicate historical mapping for `{event_type}` schema {schema_version}"
            ),
            Self::CurrentSchemaMapping {
                event_type,
                schema_version,
            } => write!(
                f,
                "historical mapping would override current `{event_type}` schema {schema_version}"
            ),
            Self::InvalidSchemaVersion {
                event_type,
                schema_version,
            } => write!(
                f,
                "event `{event_type}` has invalid schema {schema_version}; versions must be nonzero"
            ),
            Self::RenamedEvent { source, target } => write!(
                f,
                "cannot convert `{source}` to `{target}`: dispatch routes by the original event ID"
            ),
            Self::ConflictingTarget {
                event_type,
                first,
                second,
            } => write!(
                f,
                "event `{event_type}` has conflicting current schemas {first} and {second}"
            ),
        }
    }
}

impl std::error::Error for EventCodecBuildError {}

type Decoder<A> =
    Box<dyn Fn(&RecordedEvent) -> Result<<A as Aggregate>::Event, EventCodecError> + Send + Sync>;
type Encoder<A> = Box<
    dyn Fn(&<A as Aggregate>::Event, EventId) -> Result<NewEvent, EventCodecError> + Send + Sync,
>;

struct Mapping<A: Aggregate> {
    source: &'static str,
    source_version: u32,
    target: &'static str,
    target_version: u32,
    decode: Decoder<A>,
}

/// Composes one-record-to-one-event historical readers with the current JSON codec.
///
/// Conversion outputs must belong to the aggregate's event set. Historical inputs
/// only need domain metadata and JSON deserialization; they are not current variants.
/// Call [`Self::build`] once, then register the result in `AggregateCodecs`.
pub struct EventCodecBuilder<A: Aggregate> {
    mappings: Vec<Mapping<A>>,
    encoder: Option<Encoder<A>>,
}

impl<A: Aggregate> Default for EventCodecBuilder<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Aggregate> EventCodecBuilder<A> {
    pub const fn new() -> Self {
        Self {
            mappings: Vec::new(),
            encoder: None,
        }
    }

    /// Adds an infallible conversion from a historical JSON payload.
    #[must_use]
    pub fn upcast<Old, Current>(
        self,
        convert: impl Fn(Old) -> Current + Send + Sync + 'static,
    ) -> Self
    where
        Old: DomainEvent + DeserializeOwned,
        Current: DomainEvent,
        A::Event: Event + From<Current> + EventVariant<Current>,
    {
        self.try_upcast(move |old| Ok::<_, Infallible>(convert(old)))
    }

    /// Adds a conversion that can reject historical facts it cannot represent faithfully.
    #[must_use]
    pub fn try_upcast<Old, Current, E>(
        mut self,
        convert: impl Fn(Old) -> Result<Current, E> + Send + Sync + 'static,
    ) -> Self
    where
        Old: DomainEvent + DeserializeOwned,
        Current: DomainEvent,
        E: fmt::Display,
        A::Event: Event + From<Current> + EventVariant<Current>,
    {
        self.mappings.push(Mapping {
            source: Old::LOCAL_ID,
            source_version: Old::SCHEMA_VERSION,
            target: Current::LOCAL_ID,
            target_version: Current::SCHEMA_VERSION,
            decode: Box::new(move |recorded| {
                let old = rostfrei_core::__private::decode_json::<Old>(recorded.payload())
                    .map_err(|error| conversion_context::<Current>(recorded, &error))?;
                let current = convert(old).map_err(|error| {
                    conversion_context::<Current>(
                        recorded,
                        &EventCodecError::new(
                            EventCodecErrorKind::ConversionFailed,
                            error.to_string(),
                        ),
                    )
                })?;
                let event = A::Event::from(current);
                if EventVariant::<Current>::event(&event).is_none()
                    || event.event_type() != Current::LOCAL_ID
                    || event.schema_version() != Current::SCHEMA_VERSION
                {
                    return Err(conversion_context::<Current>(
                        recorded,
                        &EventCodecError::new(
                            EventCodecErrorKind::InvalidEnvelope,
                            "aggregate conversion returned a different event variant or schema",
                        ),
                    ));
                }
                Ok(event)
            }),
        });
        self
    }

    /// Selects an application-owned write format, independently of historical readers.
    /// The encoder must preserve the event ID and event type; it may choose a different schema.
    #[must_use]
    pub fn with_encoder(
        mut self,
        encoder: impl Fn(&A::Event, EventId) -> Result<NewEvent, EventCodecError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.encoder = Some(Box::new(encoder));
        self
    }

    /// Validates mappings and prepares immutable lookup tables once, before use.
    pub fn build(self) -> Result<HistoricalEventCodec<A>, EventCodecBuildError>
    where
        A::Event: Event,
    {
        let mut targets = HashMap::new();
        for mapping in &self.mappings {
            for (event_type, schema_version) in [
                (mapping.source, mapping.source_version),
                (mapping.target, mapping.target_version),
            ] {
                if schema_version == 0 {
                    return Err(EventCodecBuildError::InvalidSchemaVersion {
                        event_type,
                        schema_version,
                    });
                }
            }
            if mapping.source != mapping.target {
                return Err(EventCodecBuildError::RenamedEvent {
                    source: mapping.source,
                    target: mapping.target,
                });
            }
            if let Some(first) = targets.insert(mapping.target, mapping.target_version)
                && first != mapping.target_version
            {
                return Err(EventCodecBuildError::ConflictingTarget {
                    event_type: mapping.target,
                    first,
                    second: mapping.target_version,
                });
            }
        }
        let mut readers: HashMap<&'static str, HashMap<u32, Decoder<A>>> = HashMap::new();
        for mapping in self.mappings {
            if targets.get(mapping.source) == Some(&mapping.source_version) {
                return Err(EventCodecBuildError::CurrentSchemaMapping {
                    event_type: mapping.source,
                    schema_version: mapping.source_version,
                });
            }
            if readers
                .entry(mapping.source)
                .or_default()
                .insert(mapping.source_version, mapping.decode)
                .is_some()
            {
                return Err(EventCodecBuildError::DuplicateMapping {
                    event_type: mapping.source,
                    schema_version: mapping.source_version,
                });
            }
        }
        Ok(HistoricalEventCodec {
            readers,
            encoder: self.encoder,
        })
    }
}

/// A built, explicitly registered codec. Decoding never rebuilds its reader table.
pub struct HistoricalEventCodec<A: Aggregate> {
    readers: HashMap<&'static str, HashMap<u32, Decoder<A>>>,
    encoder: Option<Encoder<A>>,
}

impl<A: Aggregate> EventCodec<A> for HistoricalEventCodec<A>
where
    A::Event: Event,
{
    fn encode(&self, event: &A::Event, id: EventId) -> Result<NewEvent, EventCodecError> {
        let Some(encoder) = &self.encoder else {
            return <JsonEventCodec as EventCodec<A>>::encode(&JsonEventCodec, event, id);
        };
        let output = encoder(event, id.clone())?;
        if output.event_id() != &id || output.event_type() != event.event_type() {
            return Err(EventCodecError::new(
                EventCodecErrorKind::InvalidEnvelope,
                "custom event encoder must preserve the supplied event ID and event type",
            ));
        }
        Ok(output)
    }

    fn decode(&self, event: &RecordedEvent) -> Result<A::Event, EventCodecError> {
        if let Some(decode) = self
            .readers
            .get(event.event_type())
            .and_then(|versions| versions.get(&event.schema_version()))
        {
            return decode(event);
        }
        <JsonEventCodec as EventCodec<A>>::decode(&JsonEventCodec, event)
            .map_err(|error| record_context(event, &error))
    }
}

fn record_context(event: &RecordedEvent, error: &EventCodecError) -> EventCodecError {
    EventCodecError::new(
        error.kind(),
        format!(
            "event `{}` in `{}`: `{}` schema {}: {}",
            event.event_id(),
            event.stream_id(),
            event.event_type(),
            event.schema_version(),
            error.message(),
        ),
    )
}

fn conversion_context<Current: DomainEvent>(
    event: &RecordedEvent,
    error: &EventCodecError,
) -> EventCodecError {
    record_context(
        event,
        &EventCodecError::new(
            error.kind(),
            format!(
                "conversion to `{}` schema {}: {}",
                Current::LOCAL_ID,
                Current::SCHEMA_VERSION,
                error.message(),
            ),
        ),
    )
}
