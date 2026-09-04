use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
    str::FromStr,
    time::Duration,
};

use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, ExecutionMetadata, MAX_BATCH_PAYLOAD_LEN,
    MAX_EVENT_PAYLOAD_LEN, MAX_EVENT_TYPE_LEN, MAX_EVENTS_PER_BATCH, OperationId, StreamId,
};
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer},
    ser::{SerializeStruct, Serializer},
};
use serde_json::Value;
use thiserror::Error;

use crate::{CorrelationCommandOutcome, CorrelationEvent, CorrelationEventKind};

const TEST_DEFINITION_SCHEMA_VERSION: u32 = 1;
const MAX_TEST_TIMEOUT_MILLIS: u64 = 60_000;
const MAX_TEST_DEFINITIONS: usize = 256;
const MAX_TEST_DEFINITION_BYTES: usize = 1024 * 1024;
const MAX_TEST_REPOSITORY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestDefinition {
    #[serde(deserialize_with = "deserialize_definition_schema_version")]
    pub schema_version: u32,
    #[serde(deserialize_with = "deserialize_test_id")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub name: String,
    pub given: TestGiven,
    pub when: TestWhen,
    pub then: TestThen,
}

impl TestDefinition {
    pub fn from_yaml(yaml: impl AsRef<[u8]>) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_slice(yaml.as_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestGiven {
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub fixture: String,
}

pub const FIXTURE_OPERATION_ID_PREFIX: &str = "fixture-";
pub const MAX_FIXTURE_STREAMS: usize = 32;
pub const MAX_FIXTURE_EVENTS: usize = 256;
pub const MAX_FIXTURE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_EXPOSED_FIXTURE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestFixture {
    pub name: String,
    pub revision: String,
    pub streams: Vec<TestFixtureStream>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestFixtureStream {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub events: Vec<TestFixtureEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestFixtureEvent {
    pub event_type: String,
    pub schema_version: u32,
    pub stream_version: u64,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TestFixtureError {
    #[error("fixture name and revision must not be empty")]
    EmptyIdentity,
    #[error("fixture must contain between 1 and {MAX_FIXTURE_STREAMS} streams")]
    InvalidStreamCount,
    #[error("fixture contains duplicate stream `{aggregate_type}/{aggregate_id}`")]
    DuplicateStream {
        aggregate_type: String,
        aggregate_id: String,
    },
    #[error("fixture stream identities and event types must not be empty")]
    EmptyStreamIdentity,
    #[error("fixture must contain between 1 and {MAX_FIXTURE_EVENTS} events")]
    InvalidEventCount,
    #[error("fixture event schema versions must be greater than zero")]
    InvalidSchemaVersion,
    #[error("fixture stream versions must be contiguous and start at 1")]
    InvalidStreamVersion,
    #[error("fixture event payload is not valid JSON: {0}")]
    InvalidPayload(String),
    #[error("fixture payloads exceed {MAX_FIXTURE_PAYLOAD_BYTES} bytes")]
    PayloadTooLarge,
}

impl TestFixture {
    pub fn validate(&self) -> Result<(), TestFixtureError> {
        if self.name.trim().is_empty() || ContentFingerprint::from_hex(&self.revision).is_err() {
            return Err(TestFixtureError::EmptyIdentity);
        }
        if self.streams.is_empty() || self.streams.len() > MAX_FIXTURE_STREAMS {
            return Err(TestFixtureError::InvalidStreamCount);
        }
        let mut streams = std::collections::BTreeSet::new();
        let mut event_count = 0_usize;
        let mut payload_bytes = 0_usize;
        for stream in &self.streams {
            if stream.aggregate_type.trim().is_empty() || stream.aggregate_id.trim().is_empty() {
                return Err(TestFixtureError::EmptyStreamIdentity);
            }
            if !streams.insert((&stream.aggregate_type, &stream.aggregate_id)) {
                return Err(TestFixtureError::DuplicateStream {
                    aggregate_type: stream.aggregate_type.clone(),
                    aggregate_id: stream.aggregate_id.clone(),
                });
            }
            if stream.events.is_empty() || stream.events.len() > MAX_EVENTS_PER_BATCH {
                return Err(TestFixtureError::InvalidEventCount);
            }
            let mut stream_payload_bytes = 0_usize;
            event_count = event_count
                .checked_add(stream.events.len())
                .filter(|count| *count <= MAX_FIXTURE_EVENTS)
                .ok_or(TestFixtureError::InvalidEventCount)?;
            for (index, event) in stream.events.iter().enumerate() {
                if event.event_type.is_empty()
                    || event.event_type.len() > MAX_EVENT_TYPE_LEN
                    || event.event_type.trim() != event.event_type
                    || event.event_type.chars().any(char::is_control)
                {
                    return Err(TestFixtureError::EmptyStreamIdentity);
                }
                if event.schema_version == 0 {
                    return Err(TestFixtureError::InvalidSchemaVersion);
                }
                let expected_version = u64::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .ok_or(TestFixtureError::InvalidStreamVersion)?;
                if event.stream_version != expected_version {
                    return Err(TestFixtureError::InvalidStreamVersion);
                }
                let bytes = serde_json::to_vec(&event.payload)
                    .map_err(|error| TestFixtureError::InvalidPayload(error.to_string()))?;
                if bytes.len() > MAX_EVENT_PAYLOAD_LEN {
                    return Err(TestFixtureError::PayloadTooLarge);
                }
                stream_payload_bytes = stream_payload_bytes
                    .checked_add(bytes.len())
                    .filter(|bytes| *bytes <= MAX_BATCH_PAYLOAD_LEN)
                    .ok_or(TestFixtureError::PayloadTooLarge)?;
                payload_bytes = payload_bytes
                    .checked_add(bytes.len())
                    .filter(|bytes| *bytes <= MAX_FIXTURE_PAYLOAD_BYTES)
                    .ok_or(TestFixtureError::PayloadTooLarge)?;
            }
        }
        Ok(())
    }

    pub fn materialize(&self) -> Result<MaterializedTestFixture, TestFixtureError> {
        self.validate()?;
        Ok(MaterializedTestFixture {
            name: self.name.clone(),
            revision: self.revision.clone(),
            streams: self
                .streams
                .iter()
                .map(|stream| materialize_stream(self, stream))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializedTestFixture {
    pub name: String,
    pub revision: String,
    pub streams: Vec<MaterializedFixtureStream>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializedFixtureStream {
    pub aggregate_type: String,
    pub aggregate_id: String,
    #[serde(skip_serializing)]
    pub operation_id: String,
    #[serde(skip_serializing)]
    pub commit_id: String,
    #[serde(skip_serializing)]
    pub operation_fingerprint: String,
    pub events: Vec<MaterializedFixtureEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializedFixtureEvent {
    pub event_id: String,
    pub event_type: String,
    pub schema_version: u32,
    pub stream_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

fn fixture_identity(parts: &[&str]) -> String {
    let mut framed = Vec::new();
    framed.extend_from_slice(b"rostfrei:test-fixture:v1");
    for part in parts {
        let length = u64::try_from(part.len()).unwrap_or(u64::MAX);
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(part.as_bytes());
    }
    ContentFingerprint::digest(framed).to_string()
}

fn materialize_stream(
    fixture: &TestFixture,
    stream: &TestFixtureStream,
) -> Result<MaterializedFixtureStream, TestFixtureError> {
    let operation_id = format!(
        "{FIXTURE_OPERATION_ID_PREFIX}{}",
        fixture_identity(&[
            "operation",
            &fixture.name,
            &fixture.revision,
            &stream.aggregate_type,
            &stream.aggregate_id,
        ])
    );
    let fingerprint_document = stream
        .events
        .iter()
        .map(|event| {
            serde_json::json!({
                "eventType": event.event_type,
                "schemaVersion": event.schema_version,
                "payload": event.payload,
            })
        })
        .collect::<Vec<_>>();
    let operation_fingerprint = ContentFingerprint::digest(
        serde_json::to_vec(&fingerprint_document)
            .map_err(|error| TestFixtureError::InvalidPayload(error.to_string()))?,
    );
    let metadata = ExecutionMetadata::new(
        StreamId::new(
            AggregateType::new(&stream.aggregate_type)
                .map_err(|_| TestFixtureError::EmptyStreamIdentity)?,
            AggregateId::new(&stream.aggregate_id)
                .map_err(|_| TestFixtureError::EmptyStreamIdentity)?,
        ),
        OperationId::new(&operation_id).map_err(|_| TestFixtureError::EmptyStreamIdentity)?,
        operation_fingerprint,
    );
    let events = stream
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let ordinal = u32::try_from(index).map_err(|_| TestFixtureError::InvalidEventCount)?;
            Ok(MaterializedFixtureEvent {
                event_id: metadata.event_id(ordinal).to_string(),
                event_type: event.event_type.clone(),
                schema_version: event.schema_version,
                stream_version: event.stream_version,
                payload: Some(event.payload.clone()),
            })
        })
        .collect::<Result<Vec<_>, TestFixtureError>>()?;
    Ok(MaterializedFixtureStream {
        aggregate_type: stream.aggregate_type.clone(),
        aggregate_id: stream.aggregate_id.clone(),
        operation_id,
        commit_id: metadata.commit_id().to_string(),
        operation_fingerprint: operation_fingerprint.to_string(),
        events,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestWhen {
    pub command: TestCommand,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestCommand {
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub name: String,
    #[serde(deserialize_with = "deserialize_positive_schema_version")]
    pub schema_version: u32,
    pub aggregate: TestAggregate,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestAggregate {
    #[serde(rename = "type", deserialize_with = "deserialize_nonempty")]
    pub aggregate_type: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestThen {
    pub outcome: TestOutcome,
    pub within: TestTimeout,
    #[serde(default)]
    pub trace: TestTrace,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestTrace {
    #[serde(default)]
    pub contains: Vec<TraceExpectation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TraceExpectation {
    DomainEvent {
        #[serde(deserialize_with = "deserialize_nonempty")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_schema_version")]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    IntegrationEvent {
        #[serde(deserialize_with = "deserialize_nonempty")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_schema_version")]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
}

impl TraceExpectation {
    pub fn matches(&self, event: &CorrelationEvent) -> bool {
        trace_expectation_matches(self, event)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TestOutcome {
    Accepted,
    Rejected(TestRejection),
}

impl TestOutcome {
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }

    pub const fn rejection(&self) -> Option<&TestRejection> {
        match self {
            Self::Accepted => None,
            Self::Rejected(rejection) => Some(rejection),
        }
    }

    pub fn matches(&self, outcome: CorrelationCommandOutcome, result: Option<&Value>) -> bool {
        outcome_matches(self, outcome, result)
    }
}

impl Serialize for TestOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Accepted => serializer.serialize_str("accepted"),
            Self::Rejected(rejection) => {
                let mut state = serializer.serialize_struct("TestOutcome", 1)?;
                state.serialize_field("rejected", rejection)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for TestOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OutcomeDocument {
            Accepted(String),
            Rejected(RejectedOutcomeDocument),
        }

        match OutcomeDocument::deserialize(deserializer)? {
            OutcomeDocument::Accepted(value) if value == "accepted" => Ok(Self::Accepted),
            OutcomeDocument::Accepted(value) => Err(de::Error::custom(format!(
                "unsupported test outcome `{value}`"
            ))),
            OutcomeDocument::Rejected(value) => Ok(Self::Rejected(value.rejected)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectedOutcomeDocument {
    rejected: TestRejection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestRejection {
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TestTimeout(Duration);

impl TestTimeout {
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    pub const fn as_millis(self) -> u128 {
        self.0.as_millis()
    }
}

impl fmt::Display for TestTimeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let millis = self.0.as_millis();
        if millis.is_multiple_of(1000) {
            write!(formatter, "{}s", millis / 1000)
        } else {
            write!(formatter, "{millis}ms")
        }
    }
}

impl FromStr for TestTimeout {
    type Err = TestTimeoutParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (number, multiplier) = value
            .strip_suffix("ms")
            .map_or_else(
                || value.strip_suffix('s').map(|number| (number, 1000)),
                |number| Some((number, 1)),
            )
            .ok_or(TestTimeoutParseError::InvalidFormat)?;
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(TestTimeoutParseError::InvalidFormat);
        }
        let millis = number
            .parse::<u64>()
            .ok()
            .and_then(|number| number.checked_mul(multiplier))
            .filter(|millis| (1..=MAX_TEST_TIMEOUT_MILLIS).contains(millis))
            .ok_or(TestTimeoutParseError::OutOfRange)?;
        Ok(Self(Duration::from_millis(millis)))
    }
}

impl Serialize for TestTimeout {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TestTimeout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TestTimeoutParseError {
    #[error("test timeout must be a positive integer followed by `ms` or `s`")]
    InvalidFormat,
    #[error("test timeout must be between 1ms and 60s")]
    OutOfRange,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionSummary {
    pub id: String,
    pub name: String,
    pub revision: String,
    pub run_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionRevision {
    pub revision: String,
    pub definition: TestDefinition,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTestDefinition {
    pub revision: String,
    pub definition: TestDefinition,
    pub fixture: MaterializedTestFixture,
}

impl TestDefinitionRevision {
    pub fn summary(&self) -> TestDefinitionSummary {
        TestDefinitionSummary {
            id: self.definition.id.clone(),
            name: self.definition.name.clone(),
            revision: self.revision.clone(),
            run_href: format!("/tests/{}/runs", self.definition.id),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionCollection {
    pub items: Vec<TestDefinitionSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestReportStatus {
    Passed,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestExpectationResult {
    pub expectation: TraceExpectation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_event_id: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReportFailure {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub run_id: String,
    pub test_id: String,
    pub revision: String,
    pub fixture: MaterializedTestFixture,
    pub status: TestReportStatus,
    pub operation_id: String,
    pub correlation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<CorrelationCommandOutcome>,
    pub expectations: Vec<TestExpectationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<TestReportFailure>,
}

pub trait TestRepository: Send + Sync {
    fn list(&self) -> TestDefinitionCollection;

    fn get(&self, id: &str) -> Result<TestDefinitionRevision, TestRepositoryError>;
}

#[derive(Clone, Debug, Default)]
pub struct FilesystemTestRepository {
    definitions: BTreeMap<String, TestDefinitionRevision>,
}

impl FilesystemTestRepository {
    pub fn load(root: impl AsRef<Path>) -> Result<Self, TestRepositoryError> {
        let entries = fs::read_dir(root).map_err(|_| TestRepositoryError::RepositoryUnreadable)?;
        let mut candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| TestRepositoryError::RepositoryUnreadable)?;
            if is_yaml_file_name(&entry.path()) {
                candidates.push((entry.file_name(), entry.path()));
            }
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));

        if candidates.len() > MAX_TEST_DEFINITIONS {
            return Err(TestRepositoryError::TooManyDefinitions);
        }

        let mut definitions = BTreeMap::new();
        let mut source_files = BTreeMap::new();
        let mut total_bytes = 0_usize;
        let definition_read_limit = MAX_TEST_DEFINITION_BYTES
            .checked_add(1)
            .and_then(|limit| u64::try_from(limit).ok())
            .ok_or(TestRepositoryError::RepositoryTooLarge)?;
        for (file_name, path) in candidates {
            let file = file_name.to_string_lossy().into_owned();
            let file_type = fs::symlink_metadata(&path)
                .map_err(|_| TestRepositoryError::DefinitionUnreadable { file: file.clone() })?
                .file_type();
            if file_type.is_symlink() {
                return Err(TestRepositoryError::Symlink { file });
            }
            if !file_type.is_file() {
                return Err(TestRepositoryError::NotRegularFile { file });
            }

            let mut bytes = Vec::new();
            File::open(&path)
                .map_err(|_| TestRepositoryError::DefinitionUnreadable { file: file.clone() })?
                .take(definition_read_limit)
                .read_to_end(&mut bytes)
                .map_err(|_| TestRepositoryError::DefinitionUnreadable { file: file.clone() })?;
            if bytes.len() > MAX_TEST_DEFINITION_BYTES {
                return Err(TestRepositoryError::DefinitionTooLarge { file });
            }
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .filter(|total| *total <= MAX_TEST_REPOSITORY_BYTES)
                .ok_or(TestRepositoryError::RepositoryTooLarge)?;

            serde_yaml::from_slice::<serde_yaml::Value>(&bytes)
                .map_err(|_| TestRepositoryError::MalformedDefinition { file: file.clone() })?;
            let definition = TestDefinition::from_yaml(&bytes).map_err(|error| {
                TestRepositoryError::InvalidDefinition {
                    file: file.clone(),
                    message: error.to_string(),
                }
            })?;
            let revision = ContentFingerprint::digest(&bytes).to_string();
            if let Some(first) = source_files.insert(definition.id.clone(), file.clone()) {
                return Err(TestRepositoryError::DuplicateId {
                    id: definition.id,
                    first,
                    second: file,
                });
            }
            definitions.insert(
                definition.id.clone(),
                TestDefinitionRevision {
                    revision,
                    definition,
                },
            );
        }

        Ok(Self { definitions })
    }
}

impl TestRepository for FilesystemTestRepository {
    fn list(&self) -> TestDefinitionCollection {
        TestDefinitionCollection {
            items: self
                .definitions
                .values()
                .map(TestDefinitionRevision::summary)
                .collect(),
        }
    }

    fn get(&self, id: &str) -> Result<TestDefinitionRevision, TestRepositoryError> {
        self.definitions
            .get(id)
            .cloned()
            .ok_or_else(|| TestRepositoryError::NotFound(id.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TestRepositoryError {
    #[error("test repository is not configured")]
    Unavailable,
    #[error("test definition `{0}` was not found")]
    NotFound(String),
    #[error("test repository could not be read")]
    RepositoryUnreadable,
    #[error("test definition file `{file}` could not be read")]
    DefinitionUnreadable { file: String },
    #[error("test definition file `{file}` must not be a symlink")]
    Symlink { file: String },
    #[error("test definition path `{file}` is not a regular file")]
    NotRegularFile { file: String },
    #[error("test repository contains more than 256 definitions")]
    TooManyDefinitions,
    #[error("test definition file `{file}` exceeds 1 MiB")]
    DefinitionTooLarge { file: String },
    #[error("test repository definitions exceed 8 MiB in total")]
    RepositoryTooLarge,
    #[error("test definition file `{file}` contains malformed YAML")]
    MalformedDefinition { file: String },
    #[error("test definition file `{file}` is invalid: {message}")]
    InvalidDefinition { file: String, message: String },
    #[error("duplicate test definition ID `{id}` in `{first}` and `{second}`")]
    DuplicateId {
        id: String,
        first: String,
        second: String,
    },
}

pub fn payload_matches_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual| payload_matches_subset(value, actual))
        }),
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(expected, actual)| payload_matches_subset(expected, actual))
        }
        _ => expected == actual,
    }
}

pub fn trace_expectation_matches(expectation: &TraceExpectation, event: &CorrelationEvent) -> bool {
    match (expectation, &event.kind) {
        (
            TraceExpectation::DomainEvent {
                name,
                schema_version,
                payload,
            },
            CorrelationEventKind::DomainEvent {
                event_type,
                schema_version: actual_schema_version,
                payload: actual_payload,
                ..
            },
        )
        | (
            TraceExpectation::IntegrationEvent {
                name,
                schema_version,
                payload,
            },
            CorrelationEventKind::IntegrationEvent {
                event_type,
                schema_version: actual_schema_version,
                payload: actual_payload,
                ..
            },
        ) => {
            name == event_type
                && schema_version == actual_schema_version
                && payload.as_ref().is_none_or(|expected| {
                    actual_payload
                        .as_ref()
                        .is_some_and(|actual| payload_matches_subset(expected, actual))
                })
        }
        _ => false,
    }
}

pub fn outcome_matches(
    expectation: &TestOutcome,
    outcome: CorrelationCommandOutcome,
    result: Option<&Value>,
) -> bool {
    match expectation {
        TestOutcome::Accepted => outcome == CorrelationCommandOutcome::Accepted,
        TestOutcome::Rejected(expected) => {
            if outcome != CorrelationCommandOutcome::Rejected {
                return false;
            }
            let Some(rejection) = result.and_then(|result| {
                result
                    .get("rejection")
                    .or_else(|| result.as_object().map(|_| result))
            }) else {
                return false;
            };
            rejection.get("code").and_then(Value::as_str) == Some(expected.code.as_str())
                && expected.payload.as_ref().is_none_or(|payload| {
                    rejection
                        .get("details")
                        .is_some_and(|details| payload_matches_subset(payload, details))
                })
        }
    }
}

fn is_yaml_file_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "yaml" || extension == "yml")
}

fn deserialize_definition_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value == TEST_DEFINITION_SCHEMA_VERSION {
        Ok(value)
    } else {
        Err(de::Error::custom(format!(
            "unsupported test definition schema version `{value}`"
        )))
    }
}

fn deserialize_positive_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value > 0 {
        Ok(value)
    } else {
        Err(de::Error::custom(
            "schema version must be greater than zero",
        ))
    }
}

fn deserialize_nonempty<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        Err(de::Error::custom("value must not be empty"))
    } else {
        Ok(value)
    }
}

fn deserialize_test_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let mut bytes = value.bytes();
    let valid = value.len() <= 128
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(value)
    } else {
        Err(de::Error::custom(
            "test ID must contain at most 128 lowercase ASCII letters, digits, or hyphens and start with a letter or digit",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    const MINIMAL_ACCEPTED: &str = r"
schemaVersion: 1
id: rent-a-bike
name: Rent a bike
given:
  fixture: available-bike
when:
  command:
    name: start-rental
    schemaVersion: 1
    aggregate:
      type: rental/rental
      id: rental-1
    payload:
      bicycleId: bicycle-1
then:
  outcome: accepted
  within: 2s
";

    fn write_definition(root: &Path, file: &str, content: &str) {
        fs::write(root.join(file), content).expect("write test definition");
    }

    #[test]
    fn parses_and_serializes_three_document_styles() {
        let minimal = TestDefinition::from_yaml(MINIMAL_ACCEPTED).expect("minimal definition");
        assert!(minimal.then.trace.contains.is_empty());
        assert!(minimal.then.outcome.is_accepted());

        let traced = TestDefinition::from_yaml(format!(
            "{MINIMAL_ACCEPTED}\n  trace:\n    contains:\n      - kind: domain-event\n        name: rental-started\n        schemaVersion: 1\n        payload:\n          bicycleId: bicycle-1\n      - kind: integration-event\n        name: bicycle-rented\n        schemaVersion: 2\n"
        ));
        assert_eq!(
            traced.expect("traced definition").then.trace.contains.len(),
            2
        );

        let rejected = TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace(
            "outcome: accepted",
            "outcome:\n    rejected:\n      code: BIKE_UNAVAILABLE\n      payload:\n        bicycleId: bicycle-1",
        ))
        .expect("rejected definition");
        assert_eq!(
            rejected.then.outcome.rejection().expect("rejection").code,
            "BIKE_UNAVAILABLE"
        );

        let serialized = serde_yaml::to_string(&rejected).expect("serialize definition");
        assert!(serialized.contains("within: 2s"));
        assert!(serialized.contains("rejected:"));
    }

    #[test]
    fn setup_commands_are_not_part_of_the_contract() {
        let definition = MINIMAL_ACCEPTED.replace(
            "  fixture: available-bike",
            "  fixture: available-bike\n  commands: []",
        );
        assert!(TestDefinition::from_yaml(definition).is_err());

        let timeout: TestTimeout = "1000ms".parse().expect("timeout");
        assert_eq!(timeout.as_duration(), Duration::from_secs(1));
        assert_eq!(serde_yaml::to_string(&timeout).unwrap(), "1s\n");
    }

    #[test]
    fn rejects_malformed_unknown_and_invalid_documents() {
        assert!(TestDefinition::from_yaml("schemaVersion: [").is_err());
        assert!(TestDefinition::from_yaml(format!("{MINIMAL_ACCEPTED}unknown: true")).is_err());
        assert!(
            TestDefinition::from_yaml(
                MINIMAL_ACCEPTED.replace("schemaVersion: 1", "schemaVersion: 2")
            )
            .is_err()
        );
        assert!(
            TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace("within: 2s", "within: 0ms"))
                .is_err()
        );
        assert!(
            TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace("within: 2s", "within: 61s"))
                .is_err()
        );
        assert!(
            TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace("id: rent-a-bike", "id: Bad_ID"))
                .is_err()
        );
        assert!(
            TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace("name: Rent a bike", "name: ''"))
                .is_err()
        );
        assert!(
            TestDefinition::from_yaml(MINIMAL_ACCEPTED.replace(
                "schemaVersion: 1\n    aggregate:",
                "schemaVersion: 0\n    aggregate:"
            ))
            .is_err()
        );
    }

    #[test]
    fn repository_sorts_definitions_and_hashes_exact_bytes() {
        let directory = tempdir().unwrap();
        let second = MINIMAL_ACCEPTED.replace("rent-a-bike", "z-last");
        let first = MINIMAL_ACCEPTED.replace("rent-a-bike", "a-first");
        write_definition(directory.path(), "02.yaml", &second);
        write_definition(directory.path(), "01.yml", &first);
        write_definition(directory.path(), "ignored.txt", "not YAML");

        let repository = FilesystemTestRepository::load(directory.path()).unwrap();
        let collection = repository.list();
        assert_eq!(
            collection
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a-first", "z-last"]
        );
        assert_eq!(collection.items[0].run_href, "/tests/a-first/runs");
        assert_eq!(
            repository.get("a-first").unwrap().revision,
            ContentFingerprint::digest(first.as_bytes()).to_string()
        );
        assert_eq!(
            repository.get("missing"),
            Err(TestRepositoryError::NotFound("missing".to_owned()))
        );
    }

    #[test]
    fn repository_rejects_duplicate_ids_and_bad_files() {
        let duplicates = tempdir().unwrap();
        write_definition(duplicates.path(), "a.yaml", MINIMAL_ACCEPTED);
        write_definition(duplicates.path(), "b.yml", MINIMAL_ACCEPTED);
        assert!(matches!(
            FilesystemTestRepository::load(duplicates.path()),
            Err(TestRepositoryError::DuplicateId { id, first, second })
                if id == "rent-a-bike" && first == "a.yaml" && second == "b.yml"
        ));

        let malformed = tempdir().unwrap();
        write_definition(malformed.path(), "broken.yaml", "schemaVersion: [");
        assert_eq!(
            FilesystemTestRepository::load(malformed.path()).unwrap_err(),
            TestRepositoryError::MalformedDefinition {
                file: "broken.yaml".to_owned()
            }
        );

        let invalid = tempdir().unwrap();
        write_definition(
            invalid.path(),
            "invalid.yml",
            &MINIMAL_ACCEPTED.replace("within: 2s", "within: later"),
        );
        assert!(matches!(
            FilesystemTestRepository::load(invalid.path()),
            Err(TestRepositoryError::InvalidDefinition { file, .. }) if file == "invalid.yml"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn repository_rejects_yaml_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        write_definition(directory.path(), "target.txt", MINIMAL_ACCEPTED);
        symlink(
            directory.path().join("target.txt"),
            directory.path().join("linked.yaml"),
        )
        .unwrap();
        assert_eq!(
            FilesystemTestRepository::load(directory.path()).unwrap_err(),
            TestRepositoryError::Symlink {
                file: "linked.yaml".to_owned()
            }
        );
    }

    #[test]
    fn repository_enforces_file_and_definition_count_limits() {
        let oversized = tempdir().unwrap();
        fs::write(
            oversized.path().join("large.yaml"),
            vec![b' '; MAX_TEST_DEFINITION_BYTES + 1],
        )
        .unwrap();
        assert_eq!(
            FilesystemTestRepository::load(oversized.path()).unwrap_err(),
            TestRepositoryError::DefinitionTooLarge {
                file: "large.yaml".to_owned()
            }
        );

        let crowded = tempdir().unwrap();
        for index in 0..=MAX_TEST_DEFINITIONS {
            write_definition(
                crowded.path(),
                &format!("{index:03}.yaml"),
                MINIMAL_ACCEPTED,
            );
        }
        assert_eq!(
            FilesystemTestRepository::load(crowded.path()).unwrap_err(),
            TestRepositoryError::TooManyDefinitions
        );
    }

    #[test]
    fn payload_subset_matches_nested_objects_and_exact_arrays() {
        let actual = json!({
            "bike": { "id": "bike-1", "state": "available" },
            "tags": [{ "name": "road", "private": true }],
            "extra": true
        });
        assert!(payload_matches_subset(
            &json!({ "bike": { "id": "bike-1" } }),
            &actual
        ));
        assert!(payload_matches_subset(
            &json!({ "tags": [{ "name": "road" }] }),
            &actual
        ));
        assert!(!payload_matches_subset(
            &json!({ "tags": [{ "name": "road" }, { "name": "city" }] }),
            &actual
        ));
        assert!(!payload_matches_subset(
            &json!({ "bike": { "state": "rented" } }),
            &actual
        ));
    }

    #[test]
    fn trace_expectations_match_correlated_domain_and_integration_events() {
        let domain = CorrelationEvent {
            id: 1,
            correlation_id: "correlation-1".to_owned(),
            kind: CorrelationEventKind::DomainEvent {
                event_type: "rental-started".to_owned(),
                schema_version: 1,
                message_id: Some("domain-event-1".to_owned()),
                causation_id: Some("command-1".to_owned()),
                stream_version: Some(2),
                payload: Some(json!({ "bike": { "id": "bike-1", "secret": true } })),
            },
        };
        let expected_domain = TraceExpectation::DomainEvent {
            name: "rental-started".to_owned(),
            schema_version: 1,
            payload: Some(json!({ "bike": { "id": "bike-1" } })),
        };
        assert!(expected_domain.matches(&domain));

        let integration = CorrelationEvent {
            id: 2,
            correlation_id: "correlation-1".to_owned(),
            kind: CorrelationEventKind::IntegrationEvent {
                event_type: "bicycle-rented".to_owned(),
                schema_version: 2,
                message_id: Some("message-1".to_owned()),
                causation_id: Some("domain-event-1".to_owned()),
                subject: None,
                payload: None,
            },
        };
        let expected_integration = TraceExpectation::IntegrationEvent {
            name: "bicycle-rented".to_owned(),
            schema_version: 2,
            payload: None,
        };
        assert!(trace_expectation_matches(
            &expected_integration,
            &integration
        ));
        assert!(!trace_expectation_matches(&expected_domain, &integration));
    }

    #[test]
    fn expected_outcomes_match_correlation_results() {
        assert!(outcome_matches(
            &TestOutcome::Accepted,
            CorrelationCommandOutcome::Accepted,
            None
        ));
        let expected = TestOutcome::Rejected(TestRejection {
            code: "BIKE_UNAVAILABLE".to_owned(),
            payload: Some(json!({ "bicycleId": "bike-1" })),
        });
        let result = json!({
            "decision": "rejected",
            "rejection": {
                "code": "BIKE_UNAVAILABLE",
                "message": "The bicycle is unavailable",
                "details": { "bicycleId": "bike-1" }
            }
        });
        assert!(expected.matches(CorrelationCommandOutcome::Rejected, Some(&result)));
        assert!(!expected.matches(CorrelationCommandOutcome::Accepted, Some(&result)));
        assert!(!expected.matches(
            CorrelationCommandOutcome::Rejected,
            Some(&json!({
                "rejection": {
                    "code": "OTHER",
                    "details": { "bicycleId": "bike-1" }
                }
            }))
        ));
    }
    fn fixture_with_streams(streams: Vec<TestFixtureStream>) -> TestFixture {
        TestFixture {
            name: "fleet".to_owned(),
            revision: ContentFingerprint::digest("fleet-v1").to_string(),
            streams,
        }
    }

    fn fixture_stream(aggregate_id: &str, versions: &[u64]) -> TestFixtureStream {
        TestFixtureStream {
            aggregate_type: "rental/fleet".to_owned(),
            aggregate_id: aggregate_id.to_owned(),
            events: versions
                .iter()
                .map(|version| TestFixtureEvent {
                    event_type: "fleet-imported".to_owned(),
                    schema_version: 1,
                    stream_version: *version,
                    payload: json!({ "aggregate": aggregate_id, "version": version }),
                })
                .collect(),
        }
    }

    #[test]
    fn fixture_materialization_is_deterministic_and_stream_local() {
        let fixture = fixture_with_streams(vec![
            fixture_stream("north", &[1, 2]),
            fixture_stream("south", &[1]),
        ]);
        let first = fixture.materialize().unwrap();
        let second = fixture.materialize().unwrap();

        assert_eq!(first, second);
        assert_eq!(first.streams[0].events[0].stream_version, 1);
        assert_eq!(first.streams[0].events[1].stream_version, 2);
        assert_eq!(first.streams[1].events[0].stream_version, 1);
        assert_ne!(
            first.streams[0].events[0].event_id,
            first.streams[1].events[0].event_id
        );
    }

    #[test]
    fn fixture_validation_rejects_duplicates_invalid_versions_and_limits() {
        let duplicate = fixture_stream("north", &[1]);
        assert!(matches!(
            fixture_with_streams(vec![duplicate.clone(), duplicate]).validate(),
            Err(TestFixtureError::DuplicateStream { .. })
        ));
        assert_eq!(
            fixture_with_streams(vec![fixture_stream("north", &[2])]).validate(),
            Err(TestFixtureError::InvalidStreamVersion)
        );
        let too_many_streams = (0..=MAX_FIXTURE_STREAMS)
            .map(|index| fixture_stream(&format!("fleet-{index}"), &[1]))
            .collect();
        assert_eq!(
            fixture_with_streams(too_many_streams).validate(),
            Err(TestFixtureError::InvalidStreamCount)
        );
        let too_many_events =
            (1..=u64::try_from(MAX_EVENTS_PER_BATCH + 1).unwrap()).collect::<Vec<_>>();
        assert_eq!(
            fixture_with_streams(vec![fixture_stream("north", &too_many_events)]).validate(),
            Err(TestFixtureError::InvalidEventCount)
        );
        let mut oversized = fixture_stream("north", &[1]);
        oversized.events[0].payload = json!({ "value": "x".repeat(MAX_EVENT_PAYLOAD_LEN) });
        assert_eq!(
            fixture_with_streams(vec![oversized]).validate(),
            Err(TestFixtureError::PayloadTooLarge)
        );
    }
}
