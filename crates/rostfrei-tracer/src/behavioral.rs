use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
    str::FromStr,
    time::Duration,
};

use rostfrei_core::ContentFingerprint;
use schemars::{JsonSchema, Schema, schema_for};
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer},
    ser::{SerializeStruct, Serializer},
};
use serde_json::Value;
use thiserror::Error;

use crate::{
    CorrelationCommandOutcome, CorrelationEvent, CorrelationEventKind, ExpectedCommandFields,
    MessageSeriesDefinition, MessageSeriesValidationIssue,
};

const TEST_DEFINITION_SCHEMA_VERSION: u32 = 1;
const BEHAVIORAL_TEST_SCHEMA_ID: &str =
    "https://rostfrei.dev/schemas/tracer/behavioral-test-v1.schema.json";
const MAX_TEST_TIMEOUT_MILLIS: u64 = 60_000;
const MAX_TEST_SETUP_COMMANDS: usize = 32;
const MAX_TEST_DEFINITIONS: usize = 256;
const MAX_TEST_DEFINITION_BYTES: usize = 1024 * 1024;
const MAX_TEST_REPOSITORY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(
    title = "Rostfrei Tracer Behavioral Test",
    description = "A version 1 typed behavioral test. Runtime validation is authoritative for graph topology, byte-oriented identifier bounds, and the exactly-one-graph behavioral constraint.",
    extend("$id" = BEHAVIORAL_TEST_SCHEMA_ID)
)]
pub struct TestDefinition {
    #[schemars(with = "TestDefinitionSchemaVersion")]
    #[serde(deserialize_with = "deserialize_definition_schema_version")]
    schema_version: u32,
    #[schemars(with = "TestIdSchema", transform = reserve_validate_id)]
    #[serde(deserialize_with = "deserialize_test_id")]
    id: String,
    #[schemars(with = "NonEmptyBehavioralStringSchema")]
    #[serde(deserialize_with = "deserialize_nonempty")]
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(transform = disallow_null)]
    setup: Option<TestSetup>,
    #[schemars(with = "BehavioralExpectedSchema")]
    expected: MessageSeriesDefinition,
}

impl TestDefinition {
    pub fn from_json_slice(json: impl AsRef<[u8]>) -> Result<Self, TestDefinitionError> {
        let value = serde_json::from_slice(json.as_ref()).map_err(|error| {
            TestDefinitionError::new(vec![TestDefinitionValidationIssue::new(
                "malformed-json",
                "",
                error.to_string(),
            )])
        })?;
        Self::from_json_value(value)
    }

    pub fn from_json_value(value: Value) -> Result<Self, TestDefinitionError> {
        let wire = serde_json::from_value::<TestDefinitionWire>(value).map_err(|error| {
            TestDefinitionError::new(vec![TestDefinitionValidationIssue::new(
                "invalid-test-definition-document",
                "",
                error.to_string(),
            )])
        })?;
        Self::from_wire(wire)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn setup(&self) -> Option<&TestSetup> {
        self.setup.as_ref()
    }

    pub const fn expected(&self) -> &MessageSeriesDefinition {
        &self.expected
    }

    pub fn expected_graph(&self) -> Option<&crate::MessageGraphDefinition> {
        self.expected.graphs().first()
    }

    pub fn subject(&self) -> Option<ExpectedCommandFields<'_>> {
        self.expected_graph()
            .and_then(crate::MessageGraphDefinition::root_command)
    }

    fn from_wire(wire: TestDefinitionWire) -> Result<Self, TestDefinitionError> {
        let expected =
            MessageSeriesDefinition::from_json_value(wire.expected).map_err(|error| {
                TestDefinitionError::new(
                    error
                        .into_issues()
                        .into_iter()
                        .map(|issue| TestDefinitionValidationIssue::from_message_series(&issue))
                        .collect(),
                )
            })?;
        if expected.graphs().len() != 1 {
            return Err(TestDefinitionError::new(vec![
                TestDefinitionValidationIssue::new(
                    "invalid-expected-graph-count",
                    "/expected/graphs",
                    format!(
                        "behavioral test expected series must contain exactly one graph; found {}",
                        expected.graphs().len()
                    ),
                ),
            ]));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            id: wire.id,
            name: wire.name,
            setup: wire.setup,
            expected,
        })
    }
}

impl<'de> Deserialize<'de> for TestDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_wire(TestDefinitionWire::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TestDefinitionWire {
    #[serde(deserialize_with = "deserialize_definition_schema_version")]
    schema_version: u32,
    #[serde(deserialize_with = "deserialize_test_id")]
    id: String,
    #[serde(deserialize_with = "deserialize_nonempty")]
    name: String,
    #[serde(default, deserialize_with = "deserialize_present_setup")]
    setup: Option<TestSetup>,
    expected: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestSetup {
    #[schemars(with = "NonEmptyBehavioralStringSchema")]
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub fixture: String,
    #[serde(default, deserialize_with = "deserialize_setup_commands")]
    #[schemars(length(max = 32))]
    pub commands: Vec<TestCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionValidationIssue {
    code: &'static str,
    path: String,
    message: String,
}

impl TestDefinitionValidationIssue {
    fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }

    fn from_message_series(issue: &MessageSeriesValidationIssue) -> Self {
        let path = if issue.path().is_empty() {
            "/expected".to_owned()
        } else {
            format!("/expected{}", issue.path())
        };
        Self::new(issue.code(), path, issue.message())
    }

    pub const fn code(&self) -> &'static str {
        self.code
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestDefinitionError {
    issues: Vec<TestDefinitionValidationIssue>,
}

impl TestDefinitionError {
    const fn new(issues: Vec<TestDefinitionValidationIssue>) -> Self {
        Self { issues }
    }

    pub fn issues(&self) -> &[TestDefinitionValidationIssue] {
        &self.issues
    }

    pub fn into_issues(self) -> Vec<TestDefinitionValidationIssue> {
        self.issues
    }
}

impl fmt::Display for TestDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "behavioral test definition contains {} validation issue(s)",
            self.issues.len()
        )
    }
}

impl std::error::Error for TestDefinitionError {}

fn deserialize_setup_commands<'de, D>(deserializer: D) -> Result<Vec<TestCommand>, D::Error>
where
    D: Deserializer<'de>,
{
    let commands = Vec::<TestCommand>::deserialize(deserializer)?;
    if commands.len() > MAX_TEST_SETUP_COMMANDS {
        return Err(de::Error::custom(format!(
            "setup contains more than {MAX_TEST_SETUP_COMMANDS} commands"
        )));
    }
    Ok(commands)
}

fn deserialize_present_setup<'de, D>(deserializer: D) -> Result<Option<TestSetup>, D::Error>
where
    D: Deserializer<'de>,
{
    TestSetup::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestCommand {
    #[schemars(with = "NonEmptyBehavioralStringSchema")]
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub name: String,
    #[schemars(range(min = 1))]
    #[serde(deserialize_with = "deserialize_positive_schema_version")]
    pub schema_version: u32,
    pub aggregate: TestAggregate,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TestAggregate {
    #[schemars(rename = "type", with = "NonEmptyBehavioralStringSchema")]
    #[serde(rename = "type", deserialize_with = "deserialize_nonempty")]
    pub aggregate_type: String,
    #[schemars(with = "NonEmptyBehavioralStringSchema")]
    #[serde(deserialize_with = "deserialize_nonempty")]
    pub id: String,
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
    pub definition_href: String,
    pub run_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDefinitionRevision {
    pub revision: String,
    pub definition: TestDefinition,
}

impl TestDefinitionRevision {
    pub fn summary(&self) -> TestDefinitionSummary {
        TestDefinitionSummary {
            id: self.definition.id.clone(),
            name: self.definition.name.clone(),
            revision: self.revision.clone(),
            definition_href: format!("/tests/{}", self.definition.id),
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
pub struct TestReport {
    pub run_id: String,
    pub test_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub status: TestReportStatus,
    pub expected: MessageSeriesDefinition,
    pub observed: crate::ObservedMessageSeries,
    pub comparison: crate::MessageSeriesComparison,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_outcome: Option<crate::ObservedCommandOutcome>,
    pub operation_id: String,
    pub correlation_id: String,
    pub operation_href: String,
    pub operation_events_href: String,
    pub correlation_events_href: String,
    pub operation: crate::OperationSnapshot,
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
        let mut legacy_candidates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| TestRepositoryError::RepositoryUnreadable)?;
            if is_json_file_name(&entry.path()) {
                candidates.push((entry.file_name(), entry.path()));
            } else if is_legacy_yaml_file_name(&entry.path()) {
                legacy_candidates.push(entry.file_name());
            }
        }
        legacy_candidates.sort();
        if let Some(file) = legacy_candidates.first() {
            return Err(TestRepositoryError::LegacyYamlDefinition {
                file: file.to_string_lossy().into_owned(),
            });
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

            let value = serde_json::from_slice::<Value>(&bytes)
                .map_err(|_| TestRepositoryError::MalformedDefinition { file: file.clone() })?;
            let definition = TestDefinition::from_json_value(value).map_err(|error| {
                TestRepositoryError::InvalidDefinition {
                    file: file.clone(),
                    issues: error.into_issues(),
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
    #[error("test definition file `{file}` contains malformed JSON")]
    MalformedDefinition { file: String },
    #[error("legacy YAML test definition `{file}` must be migrated to canonical JSON")]
    LegacyYamlDefinition { file: String },
    #[error("test definition file `{file}` is invalid")]
    InvalidDefinition {
        file: String,
        issues: Vec<TestDefinitionValidationIssue>,
    },
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

fn is_json_file_name(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

fn is_legacy_yaml_file_name(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "yaml" | "yml"))
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
    if valid && value != "validate" {
        Ok(value)
    } else {
        Err(de::Error::custom(
            "test ID must contain at most 128 lowercase ASCII letters, digits, or hyphens, start with a letter or digit, and not be the reserved value `validate`",
        ))
    }
}

pub fn behavioral_test_definition_schema() -> Schema {
    let mut schema = schema_for!(TestDefinition);
    crate::message_series::add_unsigned_integer_maxima(&mut schema);
    schema
}

fn disallow_null(schema: &mut Schema) {
    if let Some(options) = schema
        .as_object_mut()
        .and_then(|schema| schema.get_mut("anyOf"))
        .and_then(Value::as_array_mut)
    {
        options.retain(|option| option.get("type").and_then(Value::as_str) != Some("null"));
    }
}

fn reserve_validate_id(schema: &mut Schema) {
    schema.insert("not".to_owned(), serde_json::json!({ "const": "validate" }));
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct TestDefinitionSchemaVersion(#[schemars(range(min = 1, max = 1))] u32);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct TestIdSchema(
    #[schemars(length(min = 1, max = 128), regex(pattern = r"^[a-z0-9][a-z0-9-]*$"))] String,
);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct NonEmptyBehavioralStringSchema(#[schemars(regex(pattern = r"\S"))] String);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(
    deny_unknown_fields,
    rename_all = "camelCase",
    description = "Exactly one causal graph. Runtime validation additionally enforces root, key, parent, and cycle semantics that JSON Schema cannot express."
)]
struct BehavioralExpectedSchema {
    within: BehavioralTimeoutSchema,
    settle_for: BehavioralTimeoutSchema,
    #[schemars(length(min = 1, max = 1))]
    graphs: Vec<crate::MessageGraphDefinition>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct BehavioralTimeoutSchema(
    #[schemars(regex(
        pattern = r"^0*(?:(?:[1-9]|[1-9][0-9]{1,3}|[1-5][0-9]{4}|60000)ms|(?:[1-9]|[1-5][0-9]|60)s)$"
    ))]
    String,
);

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    fn definition(id: &str) -> Value {
        json!({
            "schemaVersion": 1,
            "id": id,
            "name": "Rent a bike",
            "setup": {
                "fixture": "available-bike",
                "commands": [{
                    "name": "register-bike",
                    "schemaVersion": 1,
                    "aggregate": { "type": "rental/bicycle", "id": "bike-1" },
                    "payload": {}
                }]
            },
            "expected": {
                "within": "2s",
                "settleFor": "250ms",
                "graphs": [{
                    "nodes": [{
                        "kind": "command",
                        "key": "subject",
                        "name": "rent-bicycle",
                        "schemaVersion": 1,
                        "aggregate": { "type": "rental/rental", "id": "rental-1" },
                        "payload": { "bicycleId": "bike-1" },
                        "outcome": "accepted"
                    }]
                }]
            }
        })
    }

    fn write_definition(root: &Path, file: &str, value: &Value) -> Vec<u8> {
        let bytes = serde_json::to_vec_pretty(value).unwrap();
        fs::write(root.join(file), &bytes).unwrap();
        bytes
    }

    fn issue_codes(error: &TestDefinitionError) -> Vec<&'static str> {
        error
            .issues()
            .iter()
            .map(TestDefinitionValidationIssue::code)
            .collect()
    }

    #[test]
    fn strict_json_contract_exposes_setup_expected_and_executable_subject() {
        let value = definition("rent-a-bike");
        let parsed = TestDefinition::from_json_value(value.clone()).unwrap();

        assert_eq!(parsed.schema_version(), 1);
        assert_eq!(parsed.id(), "rent-a-bike");
        assert_eq!(parsed.setup().unwrap().commands.len(), 1);
        assert_eq!(parsed.expected().graphs().len(), 1);
        assert_eq!(parsed.subject().unwrap().key, "subject");
        assert_eq!(
            parsed.subject().unwrap().to_test_command().name,
            "rent-bicycle"
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        assert!(TestDefinition::from_json_value(definition("validate")).is_err());

        let without_setup = {
            let mut value = definition("without-setup");
            value.as_object_mut().unwrap().remove("setup");
            value
        };
        assert!(
            TestDefinition::from_json_value(without_setup)
                .unwrap()
                .setup()
                .is_none()
        );
    }

    #[test]
    fn behavioral_tests_reject_multiple_graphs_but_generic_series_accept_them() {
        let mut value = definition("two-graphs");
        let graph = value["expected"]["graphs"][0].clone();
        value["expected"]["graphs"]
            .as_array_mut()
            .unwrap()
            .push(graph);

        let generic = MessageSeriesDefinition::from_json_value(value["expected"].clone()).unwrap();
        assert_eq!(generic.graphs().len(), 2);
        let error = TestDefinition::from_json_value(value).unwrap_err();
        assert_eq!(issue_codes(&error), ["invalid-expected-graph-count"]);
        assert_eq!(error.issues()[0].path(), "/expected/graphs");
    }

    #[test]
    fn distinguishes_malformed_json_from_typed_and_semantic_validation() {
        let malformed = TestDefinition::from_json_slice(b"{").unwrap_err();
        assert_eq!(issue_codes(&malformed), ["malformed-json"]);

        let mut typed = definition("typed-invalid");
        typed["unknown"] = Value::Bool(true);
        assert_eq!(
            issue_codes(&TestDefinition::from_json_value(typed).unwrap_err()),
            ["invalid-test-definition-document"]
        );
        let mut null_setup = definition("null-setup");
        null_setup["setup"] = Value::Null;
        assert_eq!(
            issue_codes(&TestDefinition::from_json_value(null_setup).unwrap_err()),
            ["invalid-test-definition-document"]
        );

        let mut semantic = definition("semantic-invalid");
        semantic["expected"]["graphs"][0]["nodes"][0]["parentKey"] = Value::from("missing");
        let error = TestDefinition::from_json_value(semantic).unwrap_err();
        assert!(issue_codes(&error).contains(&"unresolved-parent-key"));
        assert!(
            error
                .issues()
                .iter()
                .any(|issue| issue.path().starts_with("/expected/graphs/0"))
        );
    }

    #[test]
    fn setup_and_scalar_bounds_are_strict() {
        let mut too_many = definition("too-many");
        let command = too_many["setup"]["commands"][0].clone();
        too_many["setup"]["commands"] = Value::Array(vec![command; MAX_TEST_SETUP_COMMANDS + 1]);
        assert!(TestDefinition::from_json_value(too_many).is_err());

        for (pointer, invalid) in [
            ("/schemaVersion", json!(2)),
            ("/id", json!("Bad_ID")),
            ("/name", json!("  ")),
            ("/expected/within", json!("61s")),
            ("/expected/settleFor", json!("0ms")),
        ] {
            let mut value = definition("invalid-bound");
            *value.pointer_mut(pointer).unwrap() = invalid;
            assert!(TestDefinition::from_json_value(value).is_err(), "{pointer}");
        }
    }

    #[test]
    fn repository_loads_only_sorted_immediate_json_and_hashes_exact_bytes() {
        let directory = tempdir().unwrap();
        let second = write_definition(directory.path(), "02.json", &definition("z-last"));
        let first = write_definition(directory.path(), "01.json", &definition("a-first"));
        fs::write(directory.path().join("ignored.txt"), "not: json").unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        write_definition(
            &directory.path().join("nested"),
            "nested.json",
            &definition("nested"),
        );

        let repository = FilesystemTestRepository::load(directory.path()).unwrap();
        assert_eq!(
            repository
                .list()
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a-first", "z-last"]
        );
        assert_eq!(
            repository.get("a-first").unwrap().revision,
            ContentFingerprint::digest(&first).to_string()
        );
        assert_ne!(
            ContentFingerprint::digest(first),
            ContentFingerprint::digest(second)
        );
    }

    #[test]
    fn repository_reports_legacy_yaml_that_requires_migration() {
        let directory = tempdir().unwrap();
        fs::write(directory.path().join("legacy.yaml"), "given: []").unwrap();

        assert_eq!(
            FilesystemTestRepository::load(directory.path()).unwrap_err(),
            TestRepositoryError::LegacyYamlDefinition {
                file: "legacy.yaml".to_owned(),
            }
        );
    }

    #[test]
    fn repository_distinguishes_malformed_invalid_and_duplicate_definitions() {
        let malformed = tempdir().unwrap();
        fs::write(malformed.path().join("broken.json"), "{").unwrap();
        assert_eq!(
            FilesystemTestRepository::load(malformed.path()).unwrap_err(),
            TestRepositoryError::MalformedDefinition {
                file: "broken.json".to_owned()
            }
        );

        let invalid = tempdir().unwrap();
        let mut invalid_value = definition("invalid");
        invalid_value["expected"]["graphs"] = json!([]);
        write_definition(invalid.path(), "invalid.json", &invalid_value);
        assert!(matches!(
            FilesystemTestRepository::load(invalid.path()),
            Err(TestRepositoryError::InvalidDefinition { file, issues })
                if file == "invalid.json" && !issues.is_empty()
        ));

        let duplicate = tempdir().unwrap();
        write_definition(duplicate.path(), "a.json", &definition("same"));
        write_definition(duplicate.path(), "b.json", &definition("same"));
        assert!(matches!(
            FilesystemTestRepository::load(duplicate.path()),
            Err(TestRepositoryError::DuplicateId { id, first, second })
                if id == "same" && first == "a.json" && second == "b.json"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn repository_rejects_json_symlinks_and_nonregular_candidates() {
        use std::os::unix::fs::symlink;

        let linked = tempdir().unwrap();
        write_definition(linked.path(), "target.txt", &definition("linked"));
        symlink(
            linked.path().join("target.txt"),
            linked.path().join("linked.json"),
        )
        .unwrap();
        assert_eq!(
            FilesystemTestRepository::load(linked.path()).unwrap_err(),
            TestRepositoryError::Symlink {
                file: "linked.json".to_owned()
            }
        );

        let nonregular = tempdir().unwrap();
        fs::create_dir(nonregular.path().join("directory.json")).unwrap();
        assert_eq!(
            FilesystemTestRepository::load(nonregular.path()).unwrap_err(),
            TestRepositoryError::NotRegularFile {
                file: "directory.json".to_owned()
            }
        );
    }

    #[test]
    fn repository_enforces_definition_count_and_exact_file_size_bounds() {
        let crowded = tempdir().unwrap();
        for index in 0..=MAX_TEST_DEFINITIONS {
            write_definition(
                crowded.path(),
                &format!("{index:03}.json"),
                &definition(&format!("test-{index}")),
            );
        }
        assert_eq!(
            FilesystemTestRepository::load(crowded.path()).unwrap_err(),
            TestRepositoryError::TooManyDefinitions
        );

        let oversized = tempdir().unwrap();
        fs::write(
            oversized.path().join("large.json"),
            vec![b' '; MAX_TEST_DEFINITION_BYTES + 1],
        )
        .unwrap();
        assert_eq!(
            FilesystemTestRepository::load(oversized.path()).unwrap_err(),
            TestRepositoryError::DefinitionTooLarge {
                file: "large.json".to_owned()
            }
        );

        let exact_total = tempdir().unwrap();
        for index in 0..8 {
            let mut bytes = serde_json::to_vec(&definition(&format!("exact-{index}"))).unwrap();
            bytes.resize(MAX_TEST_DEFINITION_BYTES, b' ');
            fs::write(exact_total.path().join(format!("{index}.json")), bytes).unwrap();
        }
        assert_eq!(
            FilesystemTestRepository::load(exact_total.path())
                .unwrap()
                .list()
                .items
                .len(),
            8
        );
        fs::write(exact_total.path().join("8.json"), b" ").unwrap();
        assert_eq!(
            FilesystemTestRepository::load(exact_total.path()).unwrap_err(),
            TestRepositoryError::RepositoryTooLarge
        );
    }
}
