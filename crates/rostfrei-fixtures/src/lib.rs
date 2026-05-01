//! Typed, deterministic event-store fixtures with complete message provenance.
//!
//! A [`Fixture`] retains the authored [`MessageSeries`] as its canonical
//! artifact. [`MessageSeriesEngine::apply`] validates the entire artifact but
//! persists only domain-event nodes. Each domain-event node is appended as its
//! own one-event atomic batch. All streams are preflighted before the first
//! append, but applying a multi-stream fixture is not globally atomic. The
//! deterministic plans make an interrupted application safe to retry.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    marker::PhantomData,
    sync::Arc,
};

use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, ContentFingerprint, EnvelopeError, Event, EventBatch,
    EventCodec, EventCodecError, EventStore, EventStoreError, ExecutionMetadata, ExpectedVersion,
    IdentityError, JsonEventCodec, MAX_EVENT_PAYLOAD_LEN, MAX_EVENT_TYPE_LEN, NewEvent,
    OperationId, RecordedEvent, StreamId, StreamVersion,
};
use rostfrei_messaging_core::{
    CausationId, ContractError, CorrelationId, MAX_MESSAGE_PAYLOAD_BYTES, MessageId, MessageSeries,
    MessageSeriesNode, MessageSeriesTopologyIssue, SchemaVersion,
};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::SerializeStruct,
};
use serde_json::Value;
use thiserror::Error;

/// The only fixture document schema currently supported.
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;

const FIXTURE_OPERATION_ID_PREFIX: &str = "fixture:";

/// A validated aggregate stream address used by fixture messages.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FixtureAggregate {
    aggregate_type: AggregateType,
    id: AggregateId,
}

impl FixtureAggregate {
    pub fn new(
        aggregate_type: impl Into<String>,
        id: impl Into<String>,
    ) -> Result<Self, IdentityError> {
        Ok(Self {
            aggregate_type: AggregateType::new(aggregate_type)?,
            id: AggregateId::new(id)?,
        })
    }

    pub const fn aggregate_type(&self) -> &AggregateType {
        &self.aggregate_type
    }

    pub const fn id(&self) -> &AggregateId {
        &self.id
    }

    pub fn stream_id(&self) -> StreamId {
        StreamId::new(self.aggregate_type.clone(), self.id.clone())
    }
}

impl Serialize for FixtureAggregate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("FixtureAggregate", 2)?;
        state.serialize_field("type", self.aggregate_type.as_str())?;
        state.serialize_field("id", self.id.as_str())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for FixtureAggregate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            #[serde(rename = "type")]
            aggregate_type: String,
            id: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.aggregate_type, wire.id).map_err(D::Error::custom)
    }
}

/// A message retained by a fixture's causal series.
///
/// Commands and domain events may be roots or causally linked messages.
/// Outcomes and integration events require an explicit parent; applying a
/// fixture never synthesizes or executes a command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum FixtureMessage {
    Command {
        message_id: MessageId,
        correlation_id: CorrelationId,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<MessageId>,
        name: String,
        schema_version: SchemaVersion,
        aggregate: FixtureAggregate,
        payload: Value,
    },
    CommandOutcome {
        message_id: MessageId,
        correlation_id: CorrelationId,
        causation_id: MessageId,
        outcome: Value,
    },
    DomainEvent {
        message_id: MessageId,
        correlation_id: CorrelationId,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<MessageId>,
        name: String,
        schema_version: SchemaVersion,
        aggregate: FixtureAggregate,
        stream_version: u64,
        payload: Value,
    },
    IntegrationEvent {
        message_id: MessageId,
        correlation_id: CorrelationId,
        causation_id: MessageId,
        name: String,
        schema_version: SchemaVersion,
        payload: Value,
    },
}

impl FixtureMessage {
    pub const fn is_domain_event(&self) -> bool {
        matches!(self, Self::DomainEvent { .. })
    }

    pub const fn message_id(&self) -> &MessageId {
        match self {
            Self::Command { message_id, .. }
            | Self::CommandOutcome { message_id, .. }
            | Self::DomainEvent { message_id, .. }
            | Self::IntegrationEvent { message_id, .. } => message_id,
        }
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        match self {
            Self::Command { correlation_id, .. }
            | Self::CommandOutcome { correlation_id, .. }
            | Self::DomainEvent { correlation_id, .. }
            | Self::IntegrationEvent { correlation_id, .. } => correlation_id,
        }
    }

    pub const fn causation_id(&self) -> Option<&MessageId> {
        match self {
            Self::Command { causation_id, .. } | Self::DomainEvent { causation_id, .. } => {
                causation_id.as_ref()
            }
            Self::CommandOutcome { causation_id, .. }
            | Self::IntegrationEvent { causation_id, .. } => Some(causation_id),
        }
    }

    const fn payload(&self) -> &Value {
        match self {
            Self::Command { payload, .. }
            | Self::DomainEvent { payload, .. }
            | Self::IntegrationEvent { payload, .. } => payload,
            Self::CommandOutcome { outcome, .. } => outcome,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Self::Command { name, .. }
            | Self::DomainEvent { name, .. }
            | Self::IntegrationEvent { name, .. } => Some(name),
            Self::CommandOutcome { .. } => None,
        }
    }
}

impl MessageSeriesNode for FixtureMessage {
    type CorrelationId = CorrelationId;
    type MessageId = MessageId;

    fn message_id(&self) -> &Self::MessageId {
        Self::message_id(self)
    }

    fn correlation_id(&self) -> &Self::CorrelationId {
        Self::correlation_id(self)
    }

    fn causation_id(&self) -> Option<&Self::MessageId> {
        Self::causation_id(self)
    }
}

/// A named and revisioned, immutable message-series fixture.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Fixture {
    schema_version: u32,
    id: MessageId,
    revision: MessageId,
    messages: MessageSeries<FixtureMessage>,
}

impl Fixture {
    pub fn new(
        id: impl Into<String>,
        revision: impl Into<String>,
        messages: MessageSeries<FixtureMessage>,
    ) -> Result<Self, FixtureValidationError> {
        let id = MessageId::new(id).map_err(|source| FixtureValidationError::InvalidIdentity {
            field: "id",
            source,
        })?;
        if matches!(id.as_str(), "." | "..") {
            return Err(FixtureValidationError::ReservedId { id: id.to_string() });
        }
        let revision =
            MessageId::new(revision).map_err(|source| FixtureValidationError::InvalidIdentity {
                field: "revision",
                source,
            })?;
        let fixture = Self {
            schema_version: FIXTURE_SCHEMA_VERSION,
            id,
            revision,
            messages,
        };
        fixture.validate()?;
        Ok(fixture)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn revision(&self) -> &str {
        self.revision.as_str()
    }

    pub const fn messages(&self) -> &MessageSeries<FixtureMessage> {
        &self.messages
    }

    pub fn validate(&self) -> Result<(), FixtureValidationError> {
        if self.schema_version != FIXTURE_SCHEMA_VERSION {
            return Err(FixtureValidationError::UnsupportedSchemaVersion {
                actual: self.schema_version,
            });
        }
        validate_topology(&self.messages)?;
        validate_parent_order(&self.messages)?;
        validate_outcomes(&self.messages)?;
        validate_stream_versions(&self.messages)?;
        for message in self.messages.iter() {
            validate_message(message)?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Fixture {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Wire {
            schema_version: u32,
            id: String,
            revision: String,
            messages: MessageSeries<FixtureMessage>,
        }

        let wire = Wire::deserialize(deserializer)?;
        if wire.schema_version != FIXTURE_SCHEMA_VERSION {
            return Err(D::Error::custom(
                FixtureValidationError::UnsupportedSchemaVersion {
                    actual: wire.schema_version,
                },
            ));
        }
        Self::new(wire.id, wire.revision, wire.messages).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum FixtureValidationError {
    #[error("unsupported fixture schema version `{actual}`; expected version 1")]
    UnsupportedSchemaVersion { actual: u32 },
    #[error("fixture {field} is invalid: {source}")]
    InvalidIdentity {
        field: &'static str,
        source: ContractError,
    },
    #[error("fixture id `{id}` is reserved for URI path resolution")]
    ReservedId { id: String },
    #[error("fixture topology is invalid at message `{message_id}`: {reason}")]
    InvalidTopology { message_id: String, reason: String },
    #[error("fixture parent `{parent_id}` must precede child `{child_id}`")]
    ParentMustPrecede { child_id: String, parent_id: String },
    #[error("command outcome `{outcome_id}` must name a command as its parent")]
    OutcomeParentNotCommand { outcome_id: String },
    #[error("command `{command_id}` has more than one outcome")]
    DuplicateCommandOutcome { command_id: String },
    #[error(
        "domain event `{message_id}` has stream version {actual}; expected contiguous version {expected} for `{stream_id}`"
    )]
    InvalidStreamVersion {
        message_id: String,
        stream_id: StreamId,
        actual: u64,
        expected: u64,
    },
    #[error("fixture message `{message_id}` is invalid: {reason}")]
    InvalidMessage { message_id: String, reason: String },
}

fn validate_topology(
    messages: &MessageSeries<FixtureMessage>,
) -> Result<(), FixtureValidationError> {
    let Some(issue) = messages.topology_issues().into_iter().next() else {
        return Ok(());
    };
    match issue {
        MessageSeriesTopologyIssue::UnresolvedParent { child } => {
            Err(FixtureValidationError::InvalidTopology {
                message_id: child.message_id().to_string(),
                reason: "causationId does not resolve to a fixture message".to_owned(),
            })
        }
        MessageSeriesTopologyIssue::CrossCorrelation { child, parent } => {
            Err(FixtureValidationError::InvalidTopology {
                message_id: child.message_id().to_string(),
                reason: format!(
                    "causation edge to `{}` crosses correlation IDs",
                    parent.message_id()
                ),
            })
        }
        MessageSeriesTopologyIssue::Cycle { nodes } => {
            let message_id = nodes.first().map_or_else(
                || "unknown".to_owned(),
                |node| node.message_id().to_string(),
            );
            let members = nodes
                .into_iter()
                .map(|node| node.message_id().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(FixtureValidationError::InvalidTopology {
                message_id,
                reason: format!("causation cycle contains [{members}]"),
            })
        }
        _ => Err(FixtureValidationError::InvalidTopology {
            message_id: "unknown".to_owned(),
            reason: "message series contains an unsupported topology issue".to_owned(),
        }),
    }
}

fn validate_parent_order(
    messages: &MessageSeries<FixtureMessage>,
) -> Result<(), FixtureValidationError> {
    let indexes = messages
        .iter()
        .enumerate()
        .map(|(index, message)| (message.message_id(), index))
        .collect::<HashMap<_, _>>();
    for (child_index, child) in messages.iter().enumerate() {
        let Some(parent_id) = child.causation_id() else {
            continue;
        };
        let Some(parent_index) = indexes.get(parent_id) else {
            continue;
        };
        if *parent_index >= child_index {
            return Err(FixtureValidationError::ParentMustPrecede {
                child_id: child.message_id().to_string(),
                parent_id: parent_id.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_outcomes(
    messages: &MessageSeries<FixtureMessage>,
) -> Result<(), FixtureValidationError> {
    let mut commands_with_outcomes = HashSet::new();
    for message in messages.iter() {
        let FixtureMessage::CommandOutcome {
            message_id,
            causation_id,
            ..
        } = message
        else {
            continue;
        };
        if !messages
            .get(causation_id)
            .is_some_and(|parent| matches!(parent, FixtureMessage::Command { .. }))
        {
            return Err(FixtureValidationError::OutcomeParentNotCommand {
                outcome_id: message_id.to_string(),
            });
        }
        if !commands_with_outcomes.insert(causation_id) {
            return Err(FixtureValidationError::DuplicateCommandOutcome {
                command_id: causation_id.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_stream_versions(
    messages: &MessageSeries<FixtureMessage>,
) -> Result<(), FixtureValidationError> {
    let mut expected_versions = BTreeMap::<StreamId, u64>::new();
    for message in messages.iter() {
        let FixtureMessage::DomainEvent {
            message_id,
            aggregate,
            stream_version,
            ..
        } = message
        else {
            continue;
        };
        let stream_id = aggregate.stream_id();
        let expected = expected_versions.entry(stream_id.clone()).or_insert(1);
        if stream_version != expected {
            return Err(FixtureValidationError::InvalidStreamVersion {
                message_id: message_id.to_string(),
                stream_id,
                actual: *stream_version,
                expected: *expected,
            });
        }
        *expected =
            expected
                .checked_add(1)
                .ok_or_else(|| FixtureValidationError::InvalidMessage {
                    message_id: message_id.to_string(),
                    reason: "stream version space is exhausted".to_owned(),
                })?;
    }
    Ok(())
}

fn validate_message(message: &FixtureMessage) -> Result<(), FixtureValidationError> {
    if let Some(name) = message.name() {
        let invalid = name.is_empty()
            || name.len() > MAX_EVENT_TYPE_LEN
            || name.trim() != name
            || name.chars().any(char::is_control);
        if invalid {
            return Err(FixtureValidationError::InvalidMessage {
                message_id: message.message_id().to_string(),
                reason: format!(
                    "name must be 1 to {MAX_EVENT_TYPE_LEN} bytes without surrounding whitespace or control characters"
                ),
            });
        }
    }

    let payload = canonical_json_bytes(message.payload()).map_err(|reason| {
        FixtureValidationError::InvalidMessage {
            message_id: message.message_id().to_string(),
            reason,
        }
    })?;
    let maximum = if message.is_domain_event() {
        MAX_EVENT_PAYLOAD_LEN
    } else {
        MAX_MESSAGE_PAYLOAD_BYTES
    };
    if payload.len() > maximum {
        return Err(FixtureValidationError::InvalidMessage {
            message_id: message.message_id().to_string(),
            reason: format!("payload exceeds its {maximum}-byte limit"),
        });
    }
    Ok(())
}

/// A registry of typed aggregate event codecs used to apply fixtures.
#[derive(Clone, Default)]
pub struct MessageSeriesEngine {
    codecs: BTreeMap<AggregateType, Arc<dyn RegisteredAggregateCodec>>,
}

impl MessageSeriesEngine {
    pub const fn new() -> Self {
        Self {
            codecs: BTreeMap::new(),
        }
    }

    pub fn register_json<A>(&mut self) -> Result<&mut Self, FixtureCodecRegistrationError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        let aggregate_type = AggregateType::new(A::aggregate_type())
            .map_err(|source| FixtureCodecRegistrationError::InvalidAggregateType { source })?;
        if self.codecs.contains_key(&aggregate_type) {
            return Err(FixtureCodecRegistrationError::DuplicateAggregateType { aggregate_type });
        }
        self.codecs.insert(
            aggregate_type,
            Arc::new(JsonAggregateCodec::<A>(PhantomData)),
        );
        Ok(self)
    }

    fn validate_aggregate_types(&self, fixture: &Fixture) -> Result<(), FixtureApplyError> {
        for aggregate_type in fixture.messages.iter().filter_map(|message| match message {
            FixtureMessage::Command { aggregate, .. }
            | FixtureMessage::DomainEvent { aggregate, .. } => Some(aggregate.aggregate_type()),
            FixtureMessage::CommandOutcome { .. } | FixtureMessage::IntegrationEvent { .. } => None,
        }) {
            if !self.codecs.contains_key(aggregate_type) {
                return Err(FixtureApplyError::UnknownAggregateCodec {
                    aggregate_type: aggregate_type.clone(),
                });
            }
        }
        Ok(())
    }

    /// Applies only missing domain events after preflighting every stream.
    ///
    /// Every domain-event node is one atomic event-store batch. Multi-stream
    /// fixture application is deliberately not globally atomic; retrying uses
    /// the same operation, commit, event, and fingerprint identities.
    pub async fn apply(
        &self,
        store: &dyn EventStore,
        fixture: &Fixture,
    ) -> Result<FixtureApplyReport, FixtureApplyError> {
        fixture.validate()?;
        self.validate_aggregate_types(fixture)?;

        let plan = build_plan(fixture)?;
        let mut events_by_stream = BTreeMap::<StreamId, Vec<&PlannedEvent>>::new();
        for event in &plan {
            events_by_stream
                .entry(event.recorded.stream_id().clone())
                .or_default()
                .push(event);
        }

        for (stream_id, events) in &events_by_stream {
            let codec = self.codecs.get(stream_id.aggregate_type()).ok_or_else(|| {
                FixtureApplyError::UnknownAggregateCodec {
                    aggregate_type: stream_id.aggregate_type().clone(),
                }
            })?;
            codec
                .validate(stream_id, events)
                .map_err(|failure| FixtureApplyError::Codec {
                    stream_id: stream_id.clone(),
                    event_type: failure.event_type,
                    schema_version: failure.schema_version,
                    source: failure.source,
                })?;
        }

        let mut reused_prefixes = BTreeMap::<StreamId, usize>::new();
        for (stream_id, events) in &events_by_stream {
            let history = store.load(stream_id).await?;
            if history
                .iter()
                .zip(events)
                .any(|(existing, planned)| existing != &planned.recorded)
            {
                return Err(FixtureApplyError::ConflictingHistory {
                    stream_id: stream_id.clone(),
                    existing_event_count: history.len(),
                    fixture_event_count: events.len(),
                });
            }
            reused_prefixes.insert(stream_id.clone(), history.len().min(events.len()));
        }

        let mut applied = 0_usize;
        let mut reused = reused_prefixes.values().try_fold(0_usize, |total, count| {
            total
                .checked_add(*count)
                .ok_or(FixtureApplyError::EventCountOverflow)
        })?;
        for planned in plan {
            let stream_id = planned.recorded.stream_id();
            let reused_prefix = reused_prefixes.get(stream_id).copied().unwrap_or(0);
            if planned.stream_ordinal < reused_prefix {
                continue;
            }
            let previous_version = planned
                .recorded
                .stream_version()
                .value()
                .checked_sub(1)
                .ok_or(FixtureApplyError::EventCountOverflow)?;
            let expected = if previous_version == 0 {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(StreamVersion::new(previous_version))
            };
            let outcome = store.append(stream_id, expected, planned.batch).await?;
            if outcome.events() != std::slice::from_ref(&planned.recorded) {
                return Err(FixtureApplyError::UnexpectedAppendResult {
                    stream_id: stream_id.clone(),
                    operation_id: planned.recorded.operation_id().clone(),
                });
            }
            if outcome.is_exact_replay() {
                reused = reused
                    .checked_add(1)
                    .ok_or(FixtureApplyError::EventCountOverflow)?;
            } else {
                applied = applied
                    .checked_add(1)
                    .ok_or(FixtureApplyError::EventCountOverflow)?;
            }
        }

        Ok(FixtureApplyReport {
            fixture_id: fixture.id.clone(),
            fixture_revision: fixture.revision.clone(),
            total_provenance_message_count: fixture.messages.len(),
            applied_domain_event_count: applied,
            reused_domain_event_count: reused,
            fixture: fixture.clone(),
        })
    }
}

impl fmt::Debug for MessageSeriesEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MessageSeriesEngine")
            .field("aggregate_types", &self.codecs.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum FixtureCodecRegistrationError {
    #[error("fixture codec aggregate type is invalid: {source}")]
    InvalidAggregateType { source: IdentityError },
    #[error("a fixture codec is already registered for `{aggregate_type}`")]
    DuplicateAggregateType { aggregate_type: AggregateType },
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum FixtureApplyError {
    #[error(transparent)]
    InvalidFixture(#[from] FixtureValidationError),
    #[error("no fixture codec is registered for aggregate type `{aggregate_type}`")]
    UnknownAggregateCodec { aggregate_type: AggregateType },
    #[error(
        "fixture event `{event_type}` schema {schema_version} is invalid for stream `{stream_id}`: {source}"
    )]
    Codec {
        stream_id: StreamId,
        event_type: String,
        schema_version: u32,
        source: EventCodecError,
    },
    #[error("fixture identity construction failed: {source}")]
    Identity { source: IdentityError },
    #[error("fixture message identity construction failed: {source}")]
    MessageIdentity { source: ContractError },
    #[error("fixture event envelope is invalid: {source}")]
    Envelope { source: EnvelopeError },
    #[error("fixture canonicalization failed: {message}")]
    Canonicalization { message: String },
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(
        "event-store history for `{stream_id}` conflicts with the fixture prefix ({existing_event_count} existing, {fixture_event_count} fixture events)"
    )]
    ConflictingHistory {
        stream_id: StreamId,
        existing_event_count: usize,
        fixture_event_count: usize,
    },
    #[error(
        "event store returned unexpected events for fixture operation `{operation_id}` on `{stream_id}`"
    )]
    UnexpectedAppendResult {
        stream_id: StreamId,
        operation_id: OperationId,
    },
    #[error("fixture event count overflowed")]
    EventCountOverflow,
}

/// The outcome of applying a fixture, including the unchanged provenance artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureApplyReport {
    fixture_id: MessageId,
    fixture_revision: MessageId,
    total_provenance_message_count: usize,
    applied_domain_event_count: usize,
    reused_domain_event_count: usize,
    fixture: Fixture,
}

/// Exact persisted event envelopes derived from trusted fixture documents.
#[derive(Clone, Debug, Default)]
pub struct FixtureEventSet {
    events: Vec<RecordedEvent>,
}

impl FixtureEventSet {
    pub fn new(fixtures: &[Fixture]) -> Result<Self, FixtureApplyError> {
        let mut events = Vec::new();
        for fixture in fixtures {
            fixture.validate()?;
            events.extend(
                build_plan(fixture)?
                    .into_iter()
                    .map(|planned| planned.recorded),
            );
        }
        Ok(Self { events })
    }

    pub fn contains(&self, event: &RecordedEvent) -> bool {
        self.events.contains(event)
    }

    pub fn extend(&mut self, other: Self) {
        for event in other.events {
            if !self.events.contains(&event) {
                self.events.push(event);
            }
        }
    }
}

impl FixtureApplyReport {
    pub fn fixture_id(&self) -> &str {
        self.fixture_id.as_str()
    }

    pub fn fixture_revision(&self) -> &str {
        self.fixture_revision.as_str()
    }

    pub const fn total_provenance_message_count(&self) -> usize {
        self.total_provenance_message_count
    }

    pub const fn applied_domain_event_count(&self) -> usize {
        self.applied_domain_event_count
    }

    pub const fn reused_domain_event_count(&self) -> usize {
        self.reused_domain_event_count
    }

    pub const fn fixture(&self) -> &Fixture {
        &self.fixture
    }

    pub fn into_fixture(self) -> Fixture {
        self.fixture
    }
}

trait RegisteredAggregateCodec: Send + Sync {
    fn validate(
        &self,
        stream_id: &StreamId,
        events: &[&PlannedEvent],
    ) -> Result<(), CodecValidationFailure>;
}

struct JsonAggregateCodec<A>(PhantomData<fn() -> A>);

impl<A> RegisteredAggregateCodec for JsonAggregateCodec<A>
where
    A: Aggregate,
    A::Event: Event,
{
    fn validate(
        &self,
        stream_id: &StreamId,
        events: &[&PlannedEvent],
    ) -> Result<(), CodecValidationFailure> {
        let mut state = A::initial(stream_id);
        for planned in events {
            let event =
                <JsonEventCodec as EventCodec<A>>::decode(&JsonEventCodec, &planned.recorded)
                    .map_err(|source| CodecValidationFailure {
                        event_type: planned.recorded.event_type().to_owned(),
                        schema_version: planned.recorded.schema_version(),
                        source,
                    })?;
            A::apply(&mut state, &event);
        }
        Ok(())
    }
}

struct CodecValidationFailure {
    event_type: String,
    schema_version: u32,
    source: EventCodecError,
}

struct PlannedEvent {
    recorded: RecordedEvent,
    batch: EventBatch,
    stream_ordinal: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintDocument<'a> {
    format: &'static str,
    fixture_id: &'a MessageId,
    fixture_revision: &'a MessageId,
    message: &'a FixtureMessage,
}

fn build_plan(fixture: &Fixture) -> Result<Vec<PlannedEvent>, FixtureApplyError> {
    let mut plan = Vec::new();
    let mut stream_event_counts = BTreeMap::<StreamId, usize>::new();
    let mut physical_ids = HashMap::<MessageId, CausationId>::new();

    for message in fixture.messages.iter() {
        let fingerprint = message_fingerprint(fixture, message)?;
        let parent_physical_id = message
            .causation_id()
            .map(|causation_id| {
                physical_ids.get(causation_id).cloned().ok_or_else(|| {
                    FixtureApplyError::Canonicalization {
                        message: format!(
                            "validated parent `{causation_id}` has no deterministic identity"
                        ),
                    }
                })
            })
            .transpose()?;

        let physical_id = if let FixtureMessage::DomainEvent {
            correlation_id,
            name,
            schema_version,
            aggregate,
            stream_version,
            payload,
            ..
        } = message
        {
            let stream_id = aggregate.stream_id();
            let operation_id =
                OperationId::new(format!("{FIXTURE_OPERATION_ID_PREFIX}{fingerprint}"))
                    .map_err(|source| FixtureApplyError::Identity { source })?;
            let mut metadata = ExecutionMetadata::new(stream_id.clone(), operation_id, fingerprint)
                .with_correlation_id(correlation_id.clone());
            if let Some(causation_id) = parent_physical_id {
                metadata = metadata.with_causation_id(causation_id);
            }
            let event_id = metadata.event_id(0);
            let payload = canonical_json_bytes(payload)
                .map_err(|message| FixtureApplyError::Canonicalization { message })?;
            let new_event = NewEvent::new(
                event_id.clone(),
                name,
                schema_version.get(),
                payload.clone(),
            )
            .map_err(|source| FixtureApplyError::Envelope { source })?;
            let mut batch = EventBatch::new(
                metadata.commit_id().clone(),
                metadata.operation_id().clone(),
                metadata.operation_fingerprint(),
                vec![new_event],
            )
            .map_err(|source| FixtureApplyError::Envelope { source })?
            .with_correlation_id(correlation_id.clone());
            let mut recorded = RecordedEvent::new(
                stream_id.clone(),
                StreamVersion::new(*stream_version),
                event_id.clone(),
                metadata.commit_id().clone(),
                metadata.operation_id().clone(),
                metadata.operation_fingerprint(),
                name,
                schema_version.get(),
                payload,
            )
            .map_err(|source| FixtureApplyError::Envelope { source })?
            .with_correlation_id(correlation_id.clone());
            if let Some(causation_id) = metadata.causation_id() {
                batch = batch.with_causation_id(causation_id.clone());
                recorded = recorded.with_causation_id(causation_id.clone());
            }
            let stream_event_count = stream_event_counts.entry(stream_id).or_default();
            let stream_ordinal = *stream_event_count;
            *stream_event_count = stream_event_count
                .checked_add(1)
                .ok_or(FixtureApplyError::EventCountOverflow)?;
            plan.push(PlannedEvent {
                recorded,
                batch,
                stream_ordinal,
            });
            CausationId::new(event_id.as_str())
                .map_err(|source| FixtureApplyError::MessageIdentity { source })?
        } else {
            CausationId::new(format!("fixture-message:{fingerprint}"))
                .map_err(|source| FixtureApplyError::MessageIdentity { source })?
        };
        physical_ids.insert(message.message_id().clone(), physical_id);
    }

    Ok(plan)
}

fn message_fingerprint(
    fixture: &Fixture,
    message: &FixtureMessage,
) -> Result<ContentFingerprint, FixtureApplyError> {
    let value = serde_json::to_value(FingerprintDocument {
        format: "rostfrei-fixture-message-v1",
        fixture_id: &fixture.id,
        fixture_revision: &fixture.revision,
        message,
    })
    .map_err(|error| FixtureApplyError::Canonicalization {
        message: error.to_string(),
    })?;
    canonical_json_bytes(&value)
        .map(ContentFingerprint::digest)
        .map_err(|message| FixtureApplyError::Canonicalization { message })
}

fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    write_canonical_json(value, &mut output)?;
    Ok(output)
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::String(value) => {
            serde_json::to_writer(output, value).map_err(|error| error.to_string())?;
        }
        Value::Array(values) => {
            output.push(b'[');
            let mut first = true;
            for value in values {
                if first {
                    first = false;
                } else {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(name, _)| *name);
            let mut first = true;
            for (name, value) in entries {
                if first {
                    first = false;
                } else {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, name).map_err(|error| error.to_string())?;
                output.push(b':');
                write_canonical_json(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}
