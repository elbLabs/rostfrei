use std::{
    collections::{BTreeMap, HashMap, HashSet, hash_map::Entry},
    error::Error,
    fmt,
};

use rostfrei_messaging_core::{
    CommandResponseOutcome, ContractError, MAX_IDENTIFIER_BYTES, MAX_MESSAGE_SERIES_NODES,
    MessageSeries, MessageSeriesInsertOutcome, MessageSeriesNode, MessageSeriesTopologyIssue,
};
use schemars::{JsonSchema, Schema, schema_for};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;

use crate::{TestAggregate, TestOutcome, TestRejection, TestTimeout};

const MAXIMUM_EXPECTED_GRAPH_DEPTH: usize = 64;
const MAXIMUM_EXPECTED_SIBLINGS: usize = 64;
const MAXIMUM_COMPARISON_STEPS: usize = 100_000;
const DEFINITION_SCHEMA_ID: &str =
    "https://rostfrei.dev/schemas/tracer/message-series-definition-v1.schema.json";
const OBSERVATION_SCHEMA_ID: &str =
    "https://rostfrei.dev/schemas/tracer/observed-message-series-v1.schema.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(
    title = "Rostfrei Tracer Message Series Definition",
    description = "An ordered sequence of independently rooted causal message graphs.",
    extend("$id" = DEFINITION_SCHEMA_ID)
)]
pub struct MessageSeriesDefinition {
    #[schemars(with = "TimeoutSchema")]
    within: TestTimeout,
    #[schemars(with = "TimeoutSchema")]
    settle_for: TestTimeout,
    #[schemars(length(min = 1))]
    graphs: Vec<MessageGraphDefinition>,
}

impl MessageSeriesDefinition {
    pub fn try_new(
        within: TestTimeout,
        settle_for: TestTimeout,
        graphs: Vec<MessageGraphDefinition>,
    ) -> Result<Self, MessageSeriesDefinitionError> {
        if graphs.is_empty() {
            return Err(MessageSeriesDefinitionError::new(vec![
                MessageSeriesValidationIssue::new(
                    "empty-message-series",
                    "/graphs",
                    "message series definition must contain at least one graph",
                ),
            ]));
        }
        Ok(Self {
            within,
            settle_for,
            graphs,
        })
    }

    pub const fn within(&self) -> TestTimeout {
        self.within
    }

    pub const fn settle_for(&self) -> TestTimeout {
        self.settle_for
    }

    pub fn graphs(&self) -> &[MessageGraphDefinition] {
        &self.graphs
    }

    pub fn from_json_value(value: Value) -> Result<Self, MessageSeriesDefinitionError> {
        let wire = serde_json::from_value(value).map_err(|error| {
            MessageSeriesDefinitionError::new(vec![MessageSeriesValidationIssue::new(
                "invalid-message-series-document",
                "",
                error.to_string(),
            )])
        })?;
        Self::from_wire(wire)
    }

    fn from_wire(wire: MessageSeriesDefinitionWire) -> Result<Self, MessageSeriesDefinitionError> {
        let mut issues = Vec::new();
        if wire.graphs.is_empty() {
            issues.push(MessageSeriesValidationIssue::new(
                "empty-message-series",
                "/graphs",
                "message series definition must contain at least one graph",
            ));
        }

        let mut graphs = Vec::with_capacity(wire.graphs.len());
        for (index, graph) in wire.graphs.into_iter().enumerate() {
            match MessageGraphDefinition::from_wire(graph, &format!("/graphs/{index}")) {
                Ok(graph) => graphs.push(graph),
                Err(error) => issues.extend(error.into_issues()),
            }
        }
        if !issues.is_empty() {
            return Err(MessageSeriesDefinitionError::new(issues));
        }

        Ok(Self {
            within: wire.within,
            settle_for: wire.settle_for,
            graphs,
        })
    }
}

impl<'de> Deserialize<'de> for MessageSeriesDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_wire(MessageSeriesDefinitionWire::deserialize(deserializer)?)
            .map_err(D::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MessageSeriesDefinitionWire {
    within: TestTimeout,
    settle_for: TestTimeout,
    graphs: Vec<MessageGraphDefinitionWire>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MessageGraphDefinition {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<TimeoutSchema>")]
    within: Option<TestTimeout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<TimeoutSchema>")]
    settle_for: Option<TestTimeout>,
    #[schemars(
        with = "Vec<ExpectedMessageNode>",
        length(min = 1, max = 4096),
        description = "A causal graph with at most 64 parent-child levels and at most 64 direct children per node. Comparison also has a deterministic work limit for ambiguous assignments."
    )]
    nodes: MessageSeries<ExpectedMessageNode>,
}

impl MessageGraphDefinition {
    pub fn try_from_nodes<I>(
        nodes: I,
        within: Option<TestTimeout>,
        settle_for: Option<TestTimeout>,
    ) -> Result<Self, MessageSeriesDefinitionError>
    where
        I: IntoIterator<Item = ExpectedMessageNode>,
    {
        Self::build(nodes.into_iter().collect(), within, settle_for, "")
    }

    pub const fn nodes(&self) -> &MessageSeries<ExpectedMessageNode> {
        &self.nodes
    }

    /// Returns the validated root command that is executable for this graph.
    pub fn root_command(&self) -> Option<ExpectedCommandFields<'_>> {
        self.nodes
            .roots()
            .next()
            .and_then(ExpectedMessageNode::as_command)
    }

    pub const fn within_override(&self) -> Option<TestTimeout> {
        self.within
    }

    pub const fn settle_for_override(&self) -> Option<TestTimeout> {
        self.settle_for
    }

    pub fn effective_within(&self, definition: &MessageSeriesDefinition) -> TestTimeout {
        self.within.unwrap_or_else(|| definition.within())
    }

    pub fn effective_settle_for(&self, definition: &MessageSeriesDefinition) -> TestTimeout {
        self.settle_for.unwrap_or_else(|| definition.settle_for())
    }

    fn from_wire(
        wire: MessageGraphDefinitionWire,
        path: &str,
    ) -> Result<Self, MessageSeriesDefinitionError> {
        let nodes_path = graph_path(path, "nodes");
        let mut issues = Vec::new();
        let nodes = wire
            .nodes
            .into_iter()
            .enumerate()
            .map(|(index, node)| node.into_node(&format!("{nodes_path}/{index}"), &mut issues))
            .collect();
        match Self::build(nodes, wire.within, wire.settle_for, path) {
            Ok(graph) if issues.is_empty() => Ok(graph),
            Ok(_) => Err(MessageSeriesDefinitionError::new(issues)),
            Err(error) => {
                issues.extend(error.into_issues());
                Err(MessageSeriesDefinitionError::new(issues))
            }
        }
    }

    fn build(
        nodes: Vec<ExpectedMessageNode>,
        within: Option<TestTimeout>,
        settle_for: Option<TestTimeout>,
        path: &str,
    ) -> Result<Self, MessageSeriesDefinitionError> {
        let issues = validate_graph(&nodes, path);
        if !issues.is_empty() {
            return Err(MessageSeriesDefinitionError::new(issues));
        }
        let nodes = MessageSeries::try_from_nodes(nodes).map_err(|error| {
            MessageSeriesDefinitionError::new(vec![MessageSeriesValidationIssue::new(
                "invalid-message-graph",
                graph_path(path, "nodes"),
                error.to_string(),
            )])
        })?;
        Ok(Self {
            within,
            settle_for,
            nodes,
        })
    }
}

impl<'de> Deserialize<'de> for MessageGraphDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_wire(MessageGraphDefinitionWire::deserialize(deserializer)?, "")
            .map_err(D::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MessageGraphDefinitionWire {
    within: Option<TestTimeout>,
    settle_for: Option<TestTimeout>,
    nodes: Vec<ExpectedMessageNodeWire>,
}

#[derive(Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum ExpectedMessageNodeWire {
    Command {
        key: String,
        parent_key: Option<String>,
        name: String,
        schema_version: u32,
        aggregate: TestAggregateWire,
        #[serde(default, deserialize_with = "deserialize_present_payload")]
        payload: Option<Value>,
        outcome: ExpectedOutcomeWire,
    },
    DomainEvent {
        key: String,
        parent_key: Option<String>,
        name: String,
        schema_version: u32,
        #[serde(default, deserialize_with = "deserialize_present_payload")]
        payload: Option<Value>,
    },
    IntegrationEvent {
        key: String,
        parent_key: Option<String>,
        name: String,
        schema_version: u32,
        #[serde(default, deserialize_with = "deserialize_present_payload")]
        payload: Option<Value>,
    },
}

impl ExpectedMessageNodeWire {
    fn into_node(
        self,
        path: &str,
        issues: &mut Vec<MessageSeriesValidationIssue>,
    ) -> ExpectedMessageNode {
        match self {
            Self::Command {
                key,
                parent_key,
                name,
                schema_version,
                aggregate,
                payload,
                outcome,
            } => ExpectedMessageNode::Command {
                key,
                parent_key,
                name,
                schema_version,
                aggregate: aggregate.into(),
                payload,
                outcome: outcome.into_outcome(path, issues),
            },
            Self::DomainEvent {
                key,
                parent_key,
                name,
                schema_version,
                payload,
            } => ExpectedMessageNode::DomainEvent {
                key,
                parent_key,
                name,
                schema_version,
                payload,
            },
            Self::IntegrationEvent {
                key,
                parent_key,
                name,
                schema_version,
                payload,
            } => ExpectedMessageNode::IntegrationEvent {
                key,
                parent_key,
                name,
                schema_version,
                payload,
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestAggregateWire {
    #[serde(rename = "type")]
    aggregate_type: String,
    id: String,
}

impl From<TestAggregateWire> for TestAggregate {
    fn from(value: TestAggregateWire) -> Self {
        Self {
            aggregate_type: value.aggregate_type,
            id: value.id,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ExpectedOutcomeWire {
    Accepted(String),
    Rejected(RejectedOutcomeWire),
}

impl ExpectedOutcomeWire {
    fn into_outcome(
        self,
        path: &str,
        issues: &mut Vec<MessageSeriesValidationIssue>,
    ) -> TestOutcome {
        match self {
            Self::Accepted(value) if value == "accepted" => TestOutcome::Accepted,
            Self::Accepted(value) => {
                issues.push(MessageSeriesValidationIssue::new(
                    "invalid-outcome",
                    format!("{path}/outcome"),
                    format!("unsupported command outcome `{value}`"),
                ));
                TestOutcome::Accepted
            }
            Self::Rejected(value) => TestOutcome::Rejected(TestRejection {
                code: value.rejected.code,
                payload: value.rejected.payload,
            }),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectedOutcomeWire {
    rejected: ExpectedRejectionWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedRejectionWire {
    code: String,
    #[serde(default, deserialize_with = "deserialize_present_payload")]
    payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ExpectedMessageNode {
    Command {
        #[schemars(with = "NonEmptyStringSchema")]
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Option<NonEmptyStringSchema>")]
        parent_key: Option<String>,
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[schemars(with = "AggregateSchema")]
        aggregate: TestAggregate,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
        #[schemars(with = "ExpectedOutcomeSchema")]
        outcome: TestOutcome,
    },
    DomainEvent {
        #[schemars(with = "NonEmptyStringSchema")]
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Option<NonEmptyStringSchema>")]
        parent_key: Option<String>,
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
    IntegrationEvent {
        #[schemars(with = "NonEmptyStringSchema")]
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Option<NonEmptyStringSchema>")]
        parent_key: Option<String>,
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
}

impl<'de> Deserialize<'de> for ExpectedMessageNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut issues = Vec::new();
        let node = ExpectedMessageNodeWire::deserialize(deserializer)?.into_node("", &mut issues);
        issues
            .first()
            .map_or_else(|| Ok(node), |issue| Err(D::Error::custom(&issue.message)))
    }
}

fn deserialize_present_payload<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedMessageKind {
    Command,
    DomainEvent,
    IntegrationEvent,
}

/// Borrowed fields of an expected command, including its matching outcome.
#[derive(Clone, Copy, Debug)]
pub struct ExpectedCommandFields<'a> {
    pub key: &'a str,
    pub parent_key: Option<&'a str>,
    pub name: &'a str,
    pub schema_version: u32,
    pub aggregate: &'a TestAggregate,
    pub payload: &'a Value,
    pub outcome: &'a TestOutcome,
}

impl ExpectedCommandFields<'_> {
    /// Materializes the complete command accepted by the execution API.
    pub fn to_test_command(self) -> crate::TestCommand {
        crate::TestCommand {
            name: self.name.to_owned(),
            schema_version: self.schema_version,
            aggregate: self.aggregate.clone(),
            payload: self.payload.clone(),
        }
    }
}

impl ExpectedMessageNode {
    pub fn key(&self) -> &str {
        match self {
            Self::Command { key, .. }
            | Self::DomainEvent { key, .. }
            | Self::IntegrationEvent { key, .. } => key,
        }
    }

    pub fn parent_key(&self) -> Option<&str> {
        match self {
            Self::Command { parent_key, .. }
            | Self::DomainEvent { parent_key, .. }
            | Self::IntegrationEvent { parent_key, .. } => parent_key.as_deref(),
        }
    }

    pub const fn is_command(&self) -> bool {
        matches!(self, Self::Command { .. })
    }

    pub const fn kind(&self) -> ExpectedMessageKind {
        match self {
            Self::Command { .. } => ExpectedMessageKind::Command,
            Self::DomainEvent { .. } => ExpectedMessageKind::DomainEvent,
            Self::IntegrationEvent { .. } => ExpectedMessageKind::IntegrationEvent,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Command { name, .. }
            | Self::DomainEvent { name, .. }
            | Self::IntegrationEvent { name, .. } => name,
        }
    }

    pub const fn schema_version(&self) -> u32 {
        match self {
            Self::Command { schema_version, .. }
            | Self::DomainEvent { schema_version, .. }
            | Self::IntegrationEvent { schema_version, .. } => *schema_version,
        }
    }

    pub const fn payload(&self) -> Option<&Value> {
        match self {
            Self::Command { payload, .. }
            | Self::DomainEvent { payload, .. }
            | Self::IntegrationEvent { payload, .. } => payload.as_ref(),
        }
    }

    pub const fn aggregate(&self) -> Option<&TestAggregate> {
        match self {
            Self::Command { aggregate, .. } => Some(aggregate),
            Self::DomainEvent { .. } | Self::IntegrationEvent { .. } => None,
        }
    }

    pub const fn outcome(&self) -> Option<&TestOutcome> {
        match self {
            Self::Command { outcome, .. } => Some(outcome),
            Self::DomainEvent { .. } | Self::IntegrationEvent { .. } => None,
        }
    }

    pub fn as_command(&self) -> Option<ExpectedCommandFields<'_>> {
        let Self::Command {
            key,
            parent_key,
            name,
            schema_version,
            aggregate,
            payload: Some(payload),
            outcome,
        } = self
        else {
            return None;
        };
        Some(ExpectedCommandFields {
            key,
            parent_key: parent_key.as_deref(),
            name,
            schema_version: *schema_version,
            aggregate,
            payload,
            outcome,
        })
    }
}

impl MessageSeriesNode for ExpectedMessageNode {
    type CorrelationId = ();
    type MessageId = str;

    fn message_id(&self) -> &Self::MessageId {
        self.key()
    }

    fn correlation_id(&self) -> &Self::CorrelationId {
        &()
    }

    fn causation_id(&self) -> Option<&Self::MessageId> {
        self.parent_key()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSeriesValidationIssue {
    code: &'static str,
    path: String,
    message: String,
}

impl MessageSeriesValidationIssue {
    fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
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
pub struct MessageSeriesDefinitionError {
    issues: Vec<MessageSeriesValidationIssue>,
}

impl MessageSeriesDefinitionError {
    const fn new(issues: Vec<MessageSeriesValidationIssue>) -> Self {
        Self { issues }
    }

    pub fn issues(&self) -> &[MessageSeriesValidationIssue] {
        &self.issues
    }

    pub fn into_issues(self) -> Vec<MessageSeriesValidationIssue> {
        self.issues
    }
}

impl fmt::Display for MessageSeriesDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "message series definition contains {} validation issue(s)",
            self.issues.len()
        )
    }
}

impl Error for MessageSeriesDefinitionError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ObservedMessageNode {
    Command {
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        message_id: String,
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        correlation_id: String,
        #[serde(
            default,
            deserialize_with = "deserialize_optional_observed_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        #[schemars(with = "Option<IdentifierSchema>")]
        causation_id: Option<String>,
        observation_order: u64,
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[schemars(with = "AggregateSchema")]
        aggregate: TestAggregate,
        #[serde(
            default,
            deserialize_with = "deserialize_present_payload",
            skip_serializing_if = "Option::is_none"
        )]
        payload: Option<Value>,
    },
    DomainEvent {
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        message_id: String,
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        correlation_id: String,
        #[serde(
            default,
            deserialize_with = "deserialize_optional_observed_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        #[schemars(with = "Option<IdentifierSchema>")]
        causation_id: Option<String>,
        observation_order: u64,
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(with = "Option<AggregateSchema>")]
        aggregate: Option<TestAggregate>,
        #[serde(
            default,
            deserialize_with = "deserialize_present_payload",
            skip_serializing_if = "Option::is_none"
        )]
        payload: Option<Value>,
    },
    IntegrationEvent {
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        message_id: String,
        #[serde(deserialize_with = "deserialize_observed_identifier")]
        #[schemars(with = "IdentifierSchema")]
        correlation_id: String,
        #[serde(
            default,
            deserialize_with = "deserialize_optional_observed_identifier",
            skip_serializing_if = "Option::is_none"
        )]
        #[schemars(with = "Option<IdentifierSchema>")]
        causation_id: Option<String>,
        observation_order: u64,
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1, max = 4_294_967_295_u32))]
        schema_version: u32,
        #[serde(
            default,
            deserialize_with = "deserialize_present_payload",
            skip_serializing_if = "Option::is_none"
        )]
        payload: Option<Value>,
    },
}

impl ObservedMessageNode {
    pub fn command(
        message_id: impl Into<String>,
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        name: impl Into<String>,
        schema_version: u32,
        aggregate: TestAggregate,
        payload: Option<Value>,
    ) -> Self {
        Self::Command {
            message_id: message_id.into(),
            correlation_id: correlation_id.into(),
            causation_id,
            observation_order: 0,
            name: name.into(),
            schema_version,
            aggregate,
            payload,
        }
    }

    pub fn domain_event(
        message_id: impl Into<String>,
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        name: impl Into<String>,
        schema_version: u32,
        aggregate: Option<TestAggregate>,
        payload: Option<Value>,
    ) -> Self {
        Self::DomainEvent {
            message_id: message_id.into(),
            correlation_id: correlation_id.into(),
            causation_id,
            observation_order: 0,
            name: name.into(),
            schema_version,
            aggregate,
            payload,
        }
    }

    pub fn integration_event(
        message_id: impl Into<String>,
        correlation_id: impl Into<String>,
        causation_id: Option<String>,
        name: impl Into<String>,
        schema_version: u32,
        payload: Option<Value>,
    ) -> Self {
        Self::IntegrationEvent {
            message_id: message_id.into(),
            correlation_id: correlation_id.into(),
            causation_id,
            observation_order: 0,
            name: name.into(),
            schema_version,
            payload,
        }
    }

    pub fn message_id(&self) -> &str {
        match self {
            Self::Command { message_id, .. }
            | Self::DomainEvent { message_id, .. }
            | Self::IntegrationEvent { message_id, .. } => message_id,
        }
    }

    pub fn correlation_id(&self) -> &str {
        match self {
            Self::Command { correlation_id, .. }
            | Self::DomainEvent { correlation_id, .. }
            | Self::IntegrationEvent { correlation_id, .. } => correlation_id,
        }
    }

    pub fn causation_id(&self) -> Option<&str> {
        match self {
            Self::Command { causation_id, .. }
            | Self::DomainEvent { causation_id, .. }
            | Self::IntegrationEvent { causation_id, .. } => causation_id.as_deref(),
        }
    }

    pub const fn observation_order(&self) -> u64 {
        match self {
            Self::Command {
                observation_order, ..
            }
            | Self::DomainEvent {
                observation_order, ..
            }
            | Self::IntegrationEvent {
                observation_order, ..
            } => *observation_order,
        }
    }

    pub const fn kind(&self) -> ExpectedMessageKind {
        match self {
            Self::Command { .. } => ExpectedMessageKind::Command,
            Self::DomainEvent { .. } => ExpectedMessageKind::DomainEvent,
            Self::IntegrationEvent { .. } => ExpectedMessageKind::IntegrationEvent,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Command { name, .. }
            | Self::DomainEvent { name, .. }
            | Self::IntegrationEvent { name, .. } => name,
        }
    }

    pub const fn schema_version(&self) -> u32 {
        match self {
            Self::Command { schema_version, .. }
            | Self::DomainEvent { schema_version, .. }
            | Self::IntegrationEvent { schema_version, .. } => *schema_version,
        }
    }

    pub const fn aggregate(&self) -> Option<&TestAggregate> {
        match self {
            Self::Command { aggregate, .. } => Some(aggregate),
            Self::DomainEvent { aggregate, .. } => aggregate.as_ref(),
            Self::IntegrationEvent { .. } => None,
        }
    }

    pub const fn payload(&self) -> Option<&Value> {
        match self {
            Self::Command { payload, .. }
            | Self::DomainEvent { payload, .. }
            | Self::IntegrationEvent { payload, .. } => payload.as_ref(),
        }
    }

    const fn set_observation_order(&mut self, order: u64) {
        match self {
            Self::Command {
                observation_order, ..
            }
            | Self::DomainEvent {
                observation_order, ..
            }
            | Self::IntegrationEvent {
                observation_order, ..
            } => *observation_order = order,
        }
    }

    fn same_observation(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.set_observation_order(0);
        right.set_observation_order(0);
        left == right
    }

    pub const fn is_command(&self) -> bool {
        matches!(self, Self::Command { .. })
    }
}

impl MessageSeriesNode for ObservedMessageNode {
    type CorrelationId = str;
    type MessageId = str;

    fn message_id(&self) -> &Self::MessageId {
        self.message_id()
    }

    fn correlation_id(&self) -> &Self::CorrelationId {
        self.correlation_id()
    }

    fn causation_id(&self) -> Option<&Self::MessageId> {
        self.causation_id()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ObservedCommandOutcome {
    #[serde(deserialize_with = "deserialize_observed_identifier")]
    #[schemars(with = "IdentifierSchema")]
    response_message_id: String,
    #[serde(deserialize_with = "deserialize_observed_identifier")]
    #[schemars(with = "IdentifierSchema")]
    command_message_id: String,
    #[serde(deserialize_with = "deserialize_observed_identifier")]
    #[schemars(with = "IdentifierSchema")]
    correlation_id: String,
    observation_order: u64,
    #[schemars(with = "CommandResponseOutcomeSchema")]
    outcome: CommandResponseOutcome,
}

impl ObservedCommandOutcome {
    pub fn try_new(
        response_message_id: impl Into<String>,
        command_message_id: impl Into<String>,
        correlation_id: impl Into<String>,
        outcome: CommandResponseOutcome,
    ) -> Result<Self, ObservedMessageSeriesError> {
        let outcome = Self {
            response_message_id: response_message_id.into(),
            command_message_id: command_message_id.into(),
            correlation_id: correlation_id.into(),
            observation_order: 0,
            outcome,
        };
        validate_observed_outcome(&outcome)?;
        Ok(outcome)
    }

    pub fn response_message_id(&self) -> &str {
        &self.response_message_id
    }

    pub fn command_message_id(&self) -> &str {
        &self.command_message_id
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub const fn observation_order(&self) -> u64 {
        self.observation_order
    }

    pub const fn outcome(&self) -> &CommandResponseOutcome {
        &self.outcome
    }

    const fn set_observation_order(&mut self, order: u64) {
        self.observation_order = order;
    }

    fn same_observation(&self, other: &Self) -> bool {
        self.response_message_id == other.response_message_id
            && self.command_message_id == other.command_message_id
            && self.correlation_id == other.correlation_id
            && self.outcome == other.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(
    title = "Rostfrei Tracer Observed Message Series",
    description = "Insertion-ordered causal message observations with independently arriving command outcomes.",
    extend("$id" = OBSERVATION_SCHEMA_ID)
)]
pub struct ObservedMessageSeries {
    #[schemars(with = "Vec<ObservedMessageNode>", length(max = 4096))]
    messages: MessageSeries<ObservedMessageNode>,
    #[schemars(with = "Vec<ObservedCommandOutcome>", length(max = 4096))]
    command_outcomes: Vec<ObservedCommandOutcome>,
    #[serde(skip)]
    #[schemars(skip)]
    next_observation_order: u64,
}

impl ObservedMessageSeries {
    pub const fn new() -> Self {
        Self {
            messages: MessageSeries::new(),
            command_outcomes: Vec::new(),
            next_observation_order: 0,
        }
    }

    pub fn try_from_parts<M, O>(
        messages: M,
        command_outcomes: O,
    ) -> Result<Self, ObservedMessageSeriesError>
    where
        M: IntoIterator<Item = ObservedMessageNode>,
        O: IntoIterator<Item = ObservedCommandOutcome>,
    {
        let mut series = Self::new();
        for (index, message) in messages.into_iter().enumerate() {
            if index == MAX_MESSAGE_SERIES_NODES {
                return Err(ObservedMessageSeriesError::TooManyMessages);
            }
            series.insert_message(message)?;
        }
        for (index, outcome) in command_outcomes.into_iter().enumerate() {
            if index == MAX_MESSAGE_SERIES_NODES {
                return Err(ObservedMessageSeriesError::TooManyOutcomes);
            }
            series.insert_command_outcome(outcome)?;
        }
        Ok(series)
    }

    pub fn insert_message(
        &mut self,
        mut message: ObservedMessageNode,
    ) -> Result<MessageSeriesInsertOutcome, ObservedMessageSeriesError> {
        validate_observed_message(&message)?;
        if let Some(existing) = self.messages.get(message.message_id()) {
            if existing.same_observation(&message) {
                return Ok(MessageSeriesInsertOutcome::Duplicate);
            }
            return self
                .messages
                .insert(message)
                .map_err(ObservedMessageSeriesError::MessageSeries);
        }
        message.set_observation_order(self.next_observation_order);
        let inserted = self
            .messages
            .insert(message)
            .map_err(ObservedMessageSeriesError::MessageSeries)?;
        self.next_observation_order = self
            .next_observation_order
            .checked_add(1)
            .ok_or(ObservedMessageSeriesError::ObservationOrderOverflow)?;
        Ok(inserted)
    }

    pub fn insert_command_outcome(
        &mut self,
        mut outcome: ObservedCommandOutcome,
    ) -> Result<MessageSeriesInsertOutcome, ObservedMessageSeriesError> {
        validate_observed_outcome(&outcome)?;
        if let Some(existing) = self
            .command_outcomes
            .iter()
            .find(|existing| existing.command_message_id() == outcome.command_message_id())
        {
            return if existing.same_observation(&outcome) {
                Ok(MessageSeriesInsertOutcome::Duplicate)
            } else {
                Err(ObservedMessageSeriesError::OutcomeIdentityConflict)
            };
        }
        if self
            .command_outcomes
            .iter()
            .any(|existing| existing.response_message_id() == outcome.response_message_id())
        {
            return Err(ObservedMessageSeriesError::OutcomeIdentityConflict);
        }
        if self.command_outcomes.len() == MAX_MESSAGE_SERIES_NODES {
            return Err(ObservedMessageSeriesError::TooManyOutcomes);
        }
        outcome.set_observation_order(self.next_observation_order);
        self.next_observation_order = self
            .next_observation_order
            .checked_add(1)
            .ok_or(ObservedMessageSeriesError::ObservationOrderOverflow)?;
        self.command_outcomes.push(outcome);
        Ok(MessageSeriesInsertOutcome::Inserted)
    }

    pub const fn messages(&self) -> &MessageSeries<ObservedMessageNode> {
        &self.messages
    }

    pub fn command_outcomes(&self) -> &[ObservedCommandOutcome] {
        &self.command_outcomes
    }

    pub fn command_outcome(&self, command_message_id: &str) -> Option<&ObservedCommandOutcome> {
        self.command_outcomes
            .iter()
            .find(|outcome| outcome.command_message_id() == command_message_id)
    }

    pub fn topology_issues(&self) -> Vec<MessageSeriesTopologyIssue<'_, ObservedMessageNode>> {
        self.messages.topology_issues()
    }

    pub fn outcome_issues(&self) -> Vec<ObservedMessageSeriesOutcomeIssue<'_>> {
        self.command_outcomes
            .iter()
            .filter_map(|outcome| {
                let Some(message) = self.messages.get(outcome.command_message_id()) else {
                    return Some(ObservedMessageSeriesOutcomeIssue::MissingCommand { outcome });
                };
                if !message.is_command() {
                    return Some(ObservedMessageSeriesOutcomeIssue::NotACommand {
                        outcome,
                        message,
                    });
                }
                (message.correlation_id() != outcome.correlation_id()).then_some(
                    ObservedMessageSeriesOutcomeIssue::CrossCorrelation { outcome, message },
                )
            })
            .collect()
    }
}

impl Default for ObservedMessageSeries {
    fn default() -> Self {
        Self::new()
    }
}

impl<'de> Deserialize<'de> for ObservedMessageSeries {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ObservedMessageSeriesWire::deserialize(deserializer)?;
        if wire.messages.len() > MAX_MESSAGE_SERIES_NODES {
            return Err(D::Error::custom(
                ObservedMessageSeriesError::TooManyMessages,
            ));
        }
        if wire.command_outcomes.len() > MAX_MESSAGE_SERIES_NODES {
            return Err(D::Error::custom(
                ObservedMessageSeriesError::TooManyOutcomes,
            ));
        }
        Self::try_from_serialized_parts(wire.messages, wire.command_outcomes)
            .map_err(D::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ObservedMessageSeriesWire {
    messages: Vec<ObservedMessageNode>,
    command_outcomes: Vec<ObservedCommandOutcome>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservedMessageSeriesError {
    InvalidMessage { field: &'static str },
    InvalidOutcome { field: &'static str },
    MessageSeries(ContractError),
    OutcomeIdentityConflict,
    DuplicateSerializedMessage,
    DuplicateSerializedOutcome,
    TooManyMessages,
    TooManyOutcomes,
    InvalidObservationOrder,
    ObservationOrderOverflow,
}

impl fmt::Display for ObservedMessageSeriesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMessage { field } => {
                write!(formatter, "observed message {field} is invalid")
            }
            Self::InvalidOutcome { field } => {
                write!(formatter, "observed command outcome {field} is invalid")
            }
            Self::MessageSeries(error) => error.fmt(formatter),
            Self::OutcomeIdentityConflict => {
                formatter.write_str("observed command outcome identity conflicts")
            }
            Self::DuplicateSerializedMessage => {
                formatter.write_str("serialized observed message series contains a duplicate message identity")
            }
            Self::DuplicateSerializedOutcome => formatter.write_str(
                "serialized observed message series contains a duplicate command outcome",
            ),
            Self::TooManyMessages => {
                formatter.write_str("observed message series contains more than 4096 messages")
            }
            Self::TooManyOutcomes => formatter
                .write_str("observed message series contains more than 4096 command outcomes"),
            Self::InvalidObservationOrder => formatter.write_str(
                "observed message series observation orders must be unique contiguous insertion ordinals",
            ),
            Self::ObservationOrderOverflow => {
                formatter.write_str("observed message series observation order overflowed")
            }
        }
    }
}

impl ObservedMessageSeries {
    fn try_from_serialized_parts(
        messages: Vec<ObservedMessageNode>,
        command_outcomes: Vec<ObservedCommandOutcome>,
    ) -> Result<Self, ObservedMessageSeriesError> {
        let mut message_ids = HashSet::new();
        for message in &messages {
            validate_observed_message(message)?;
            if !message_ids.insert(message.message_id()) {
                return Err(ObservedMessageSeriesError::DuplicateSerializedMessage);
            }
        }
        let messages = MessageSeries::try_from_nodes(messages)
            .map_err(ObservedMessageSeriesError::MessageSeries)?;

        let mut outcomes = Vec::<ObservedCommandOutcome>::new();
        for outcome in command_outcomes {
            validate_observed_outcome(&outcome)?;
            if let Some(existing) = outcomes.iter().find(|existing| {
                existing.command_message_id() == outcome.command_message_id()
                    || existing.response_message_id() == outcome.response_message_id()
            }) {
                if existing.same_observation(&outcome) {
                    return Err(ObservedMessageSeriesError::DuplicateSerializedOutcome);
                }
                return Err(ObservedMessageSeriesError::OutcomeIdentityConflict);
            }
            outcomes.push(outcome);
        }

        let total = messages
            .len()
            .checked_add(outcomes.len())
            .ok_or(ObservedMessageSeriesError::ObservationOrderOverflow)?;
        let total_u64 = u64::try_from(total)
            .map_err(|_| ObservedMessageSeriesError::ObservationOrderOverflow)?;
        let orders = messages
            .iter()
            .map(ObservedMessageNode::observation_order)
            .chain(
                outcomes
                    .iter()
                    .map(ObservedCommandOutcome::observation_order),
            )
            .collect::<HashSet<_>>();
        if orders.len() != total || orders.iter().any(|order| *order >= total_u64) {
            return Err(ObservedMessageSeriesError::InvalidObservationOrder);
        }
        if !orders_increase(messages.iter().map(ObservedMessageNode::observation_order))
            || !orders_increase(
                outcomes
                    .iter()
                    .map(ObservedCommandOutcome::observation_order),
            )
        {
            return Err(ObservedMessageSeriesError::InvalidObservationOrder);
        }

        Ok(Self {
            messages,
            command_outcomes: outcomes,
            next_observation_order: total_u64,
        })
    }
}

fn orders_increase(orders: impl IntoIterator<Item = u64>) -> bool {
    let mut previous = None;
    for order in orders {
        if previous.is_some_and(|previous| previous >= order) {
            return false;
        }
        previous = Some(order);
    }
    true
}

impl Error for ObservedMessageSeriesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MessageSeries(error) => Some(error),
            _ => None,
        }
    }
}

fn validate_observed_message(
    message: &ObservedMessageNode,
) -> Result<(), ObservedMessageSeriesError> {
    if !is_valid_observed_identifier(message.message_id()) {
        return Err(ObservedMessageSeriesError::InvalidMessage { field: "messageId" });
    }
    if !is_valid_observed_identifier(message.correlation_id()) {
        return Err(ObservedMessageSeriesError::InvalidMessage {
            field: "correlationId",
        });
    }
    if message
        .causation_id()
        .is_some_and(|value| !is_valid_observed_identifier(value))
    {
        return Err(ObservedMessageSeriesError::InvalidMessage {
            field: "causationId",
        });
    }
    match message {
        ObservedMessageNode::Command {
            name,
            schema_version,
            aggregate,
            ..
        } => {
            validate_observed_name(name)?;
            validate_observed_schema_version(*schema_version)?;
            validate_observed_aggregate(aggregate)?;
        }
        ObservedMessageNode::DomainEvent {
            name,
            schema_version,
            aggregate,
            ..
        } => {
            validate_observed_name(name)?;
            validate_observed_schema_version(*schema_version)?;
            if let Some(aggregate) = aggregate {
                validate_observed_aggregate(aggregate)?;
            }
        }
        ObservedMessageNode::IntegrationEvent {
            name,
            schema_version,
            ..
        } => {
            validate_observed_name(name)?;
            validate_observed_schema_version(*schema_version)?;
        }
    }
    Ok(())
}

fn validate_observed_aggregate(
    aggregate: &TestAggregate,
) -> Result<(), ObservedMessageSeriesError> {
    if aggregate.aggregate_type.trim().is_empty() {
        return Err(ObservedMessageSeriesError::InvalidMessage {
            field: "aggregate.type",
        });
    }
    if aggregate.id.trim().is_empty() {
        return Err(ObservedMessageSeriesError::InvalidMessage {
            field: "aggregate.id",
        });
    }
    Ok(())
}

fn validate_observed_name(name: &str) -> Result<(), ObservedMessageSeriesError> {
    if name.trim().is_empty() {
        Err(ObservedMessageSeriesError::InvalidMessage { field: "name" })
    } else {
        Ok(())
    }
}

const fn validate_observed_schema_version(
    schema_version: u32,
) -> Result<(), ObservedMessageSeriesError> {
    if schema_version == 0 {
        Err(ObservedMessageSeriesError::InvalidMessage {
            field: "schemaVersion",
        })
    } else {
        Ok(())
    }
}

fn validate_observed_outcome(
    outcome: &ObservedCommandOutcome,
) -> Result<(), ObservedMessageSeriesError> {
    for (field, value) in [
        ("responseMessageId", outcome.response_message_id()),
        ("commandMessageId", outcome.command_message_id()),
        ("correlationId", outcome.correlation_id()),
    ] {
        if !is_valid_observed_identifier(value) {
            return Err(ObservedMessageSeriesError::InvalidOutcome { field });
        }
    }
    Ok(())
}

fn is_valid_observed_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn deserialize_observed_identifier<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if is_valid_observed_identifier(&value) {
        Ok(value)
    } else {
        Err(D::Error::custom("message identifier is invalid"))
    }
}

fn deserialize_optional_observed_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if value
        .as_deref()
        .is_some_and(|value| !is_valid_observed_identifier(value))
    {
        Err(D::Error::custom("message identifier is invalid"))
    } else {
        Ok(value)
    }
}

fn deserialize_nonempty_observed_name<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        Err(D::Error::custom("message name must not be empty"))
    } else {
        Ok(value)
    }
}

fn deserialize_positive_observed_schema_version<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value == 0 {
        Err(D::Error::custom(
            "message schema version must be greater than zero",
        ))
    } else {
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ObservedMessageSeriesOutcomeIssue<'a> {
    MissingCommand {
        outcome: &'a ObservedCommandOutcome,
    },
    NotACommand {
        outcome: &'a ObservedCommandOutcome,
        message: &'a ObservedMessageNode,
    },
    CrossCorrelation {
        outcome: &'a ObservedCommandOutcome,
        message: &'a ObservedMessageNode,
    },
}

/// Whether an expected causal graph matched all observed behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSeriesComparisonStatus {
    Passed,
    Failed,
}

/// A deterministic expected-key to observed-message identity assignment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSeriesMatch {
    pub expected_key: String,
    pub observed_message_id: String,
}

/// A machine-readable comparison failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSeriesComparisonDiagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<Value>,
}

impl MessageSeriesComparisonDiagnostic {
    fn new(
        code: &'static str,
        path: impl Into<String>,
        message: impl Into<String>,
        expected: Option<Value>,
        observed: Option<Value>,
    ) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            expected,
            observed,
        }
    }
}

/// Observation boundaries supplied by the execution loop.
///
/// Boundaries are global `observationOrder` values. `settle_started_at_order`
/// is the first order observed during settling, and `settle_completed_at_order`
/// is the first order observed after settling. They classify timing only and
/// never establish causality.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MessageSeriesComparisonContext {
    pub timed_out: bool,
    pub timeout_observation_order: Option<u64>,
    pub settle_started_at_order: Option<u64>,
    pub settle_completed_at_order: Option<u64>,
}

/// Structured output from deterministic causal graph comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSeriesComparison {
    pub status: MessageSeriesComparisonStatus,
    pub matches: Vec<MessageSeriesMatch>,
    pub diagnostics: Vec<MessageSeriesComparisonDiagnostic>,
}

/// Compares one expected graph with observed messages and command outcomes.
///
/// Complete causal assignments are preferred over locally greedy sibling
/// choices. Expected object payloads are recursive subsets; arrays retain exact
/// length and order, while scalars and explicit null are exact. An absent
/// expected payload is a wildcard. Candidate selection is ordered by content
/// and then message ID; observation order is never a causal signal or matching
/// tie-breaker.
#[allow(
    clippy::too_many_lines,
    reason = "comparison budgeting and deterministic fallback are one stateful operation"
)]
pub fn compare_message_series(
    expected: &MessageGraphDefinition,
    observed: &ObservedMessageSeries,
    context: MessageSeriesComparisonContext,
) -> MessageSeriesComparison {
    let mut diagnostics = observed_structure_diagnostics(observed);
    let mut matches = BTreeMap::<String, String>::new();
    let mut matched_ids = HashSet::<String>::new();
    let expected_by_key = expected
        .nodes()
        .iter()
        .map(|node| (node.key(), node))
        .collect::<HashMap<_, _>>();
    let mut expected_nodes = expected.nodes().iter().collect::<Vec<_>>();
    expected_nodes.sort_by_key(|node| (expected_depth(node, &expected_by_key), node.key()));
    let mut missing = Vec::new();

    let mut comparison_exhausted = false;
    match find_complete_causal_assignment(expected, observed) {
        CompleteCausalAssignment::Complete(complete) => {
            for (expected_key, observed_id) in complete {
                matched_ids.insert(observed_id.clone());
                matches.insert(expected_key, observed_id);
            }
        }
        CompleteCausalAssignment::Incomplete(mut remaining_steps) => 'fallback: {
            for expected_node in expected_nodes {
                let expected_parent_id = expected_node
                    .parent_key()
                    .and_then(|parent_key| matches.get(parent_key));
                let mut candidates = Vec::new();
                for candidate in observed.messages().iter() {
                    if !spend_comparison_step(&mut remaining_steps) {
                        comparison_exhausted = true;
                        break 'fallback;
                    }
                    if matched_ids.contains(candidate.message_id()) {
                        continue;
                    }
                    let causal_candidate = match expected_node.parent_key() {
                        None => candidate.causation_id().is_none(),
                        Some(_) => expected_parent_id.is_some_and(|parent_id| {
                            candidate.causation_id() == Some(parent_id.as_str())
                        }),
                    };
                    if causal_candidate {
                        candidates.push(candidate);
                    }
                }
                candidates
                    .sort_by_key(|candidate| candidate_rank(expected_node, candidate, observed));

                let Some(candidate) = candidates.first().copied() else {
                    if !diagnose_unmatched_expected(
                        expected_node,
                        observed,
                        &matched_ids,
                        expected_parent_id.map(String::as_str),
                        &mut diagnostics,
                        &mut remaining_steps,
                    ) {
                        comparison_exhausted = true;
                        break 'fallback;
                    }
                    missing.push(expected_node.key().to_owned());
                    continue;
                };
                matched_ids.insert(candidate.message_id().to_owned());
                matches.insert(
                    expected_node.key().to_owned(),
                    candidate.message_id().to_owned(),
                );
                let diagnostic_count = diagnostics.len();
                diagnose_matched_fields(expected_node, candidate, observed, &mut diagnostics);
                if diagnostics.len() != diagnostic_count {
                    missing.push(expected_node.key().to_owned());
                }
            }
        }
        CompleteCausalAssignment::Exhausted => comparison_exhausted = true,
    }

    if comparison_exhausted {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "comparison-work-limit-exceeded",
            "/expected",
            format!(
                "message-series comparison exceeded its deterministic {MAXIMUM_COMPARISON_STEPS}-step work limit"
            ),
            None,
            None,
        ));
    } else {
        diagnose_unexpected_observations(observed, &matched_ids, &mut diagnostics);
        diagnose_timing(observed, context, &missing, &mut diagnostics);
    }
    diagnostics.sort_by(|left, right| {
        (&left.path, left.code, &left.message).cmp(&(&right.path, right.code, &right.message))
    });
    let matches = matches
        .into_iter()
        .map(|(expected_key, observed_message_id)| MessageSeriesMatch {
            expected_key,
            observed_message_id,
        })
        .collect();
    MessageSeriesComparison {
        status: if diagnostics.is_empty() {
            MessageSeriesComparisonStatus::Passed
        } else {
            MessageSeriesComparisonStatus::Failed
        },
        matches,
        diagnostics,
    }
}

fn expected_depth(
    node: &ExpectedMessageNode,
    expected_by_key: &HashMap<&str, &ExpectedMessageNode>,
) -> usize {
    let mut depth = 0_usize;
    let mut parent = node.parent_key();
    while let Some(parent_key) = parent {
        depth = depth.saturating_add(1);
        parent = expected_by_key
            .get(parent_key)
            .and_then(|parent| parent.parent_key());
    }
    depth
}

fn find_complete_causal_assignment(
    expected: &MessageGraphDefinition,
    observed: &ObservedMessageSeries,
) -> CompleteCausalAssignment {
    CompleteMatchSearch::new(expected, observed).assignment()
}

enum CompleteCausalAssignment {
    Complete(BTreeMap<String, String>),
    Incomplete(usize),
    Exhausted,
}

struct CompleteMatchSearch<'a> {
    expected_nodes: Vec<&'a ExpectedMessageNode>,
    observed_nodes: Vec<&'a ObservedMessageNode>,
    expected_roots: Vec<usize>,
    observed_roots: Vec<usize>,
    expected_children: HashMap<String, Vec<usize>>,
    observed_children: HashMap<String, Vec<usize>>,
    compatibility: HashMap<(usize, usize), bool>,
    remaining_steps: usize,
    exhausted: bool,
    observed: &'a ObservedMessageSeries,
}

impl<'a> CompleteMatchSearch<'a> {
    fn new(expected: &'a MessageGraphDefinition, observed: &'a ObservedMessageSeries) -> Self {
        let expected_nodes = expected.nodes().iter().collect::<Vec<_>>();
        let observed_nodes = observed.messages().iter().collect::<Vec<_>>();
        let mut expected_roots = Vec::new();
        let mut observed_roots = Vec::new();
        let mut expected_children = HashMap::<String, Vec<usize>>::new();
        let mut observed_children = HashMap::<String, Vec<usize>>::new();

        for (index, node) in expected_nodes.iter().enumerate() {
            if let Some(parent_key) = node.parent_key() {
                expected_children
                    .entry(parent_key.to_owned())
                    .or_default()
                    .push(index);
            } else {
                expected_roots.push(index);
            }
        }
        for (index, node) in observed_nodes.iter().enumerate() {
            if let Some(causation_id) = node.causation_id() {
                observed_children
                    .entry(causation_id.to_owned())
                    .or_default()
                    .push(index);
            } else {
                observed_roots.push(index);
            }
        }
        for children in expected_children.values_mut() {
            children.sort_by(|left, right| {
                match (expected_nodes.get(*left), expected_nodes.get(*right)) {
                    (Some(left), Some(right)) => left.key().cmp(right.key()),
                    _ => left.cmp(right),
                }
            });
        }
        let observed_order = |left: &usize, right: &usize| match (
            observed_nodes.get(*left),
            observed_nodes.get(*right),
        ) {
            (Some(left), Some(right)) => (candidate_content_key(left, observed), left.message_id())
                .cmp(&(candidate_content_key(right, observed), right.message_id())),
            _ => left.cmp(right),
        };
        observed_roots.sort_by(observed_order);
        for children in observed_children.values_mut() {
            children.sort_by(observed_order);
        }

        Self {
            expected_nodes,
            observed_nodes,
            expected_roots,
            observed_roots,
            expected_children,
            observed_children,
            compatibility: HashMap::new(),
            remaining_steps: MAXIMUM_COMPARISON_STEPS,
            exhausted: false,
            observed,
        }
    }

    fn assignment(mut self) -> CompleteCausalAssignment {
        let Some(expected_root) = self.expected_roots.first().copied() else {
            return CompleteCausalAssignment::Incomplete(self.remaining_steps);
        };
        for observed_root in self.observed_roots.clone() {
            if !self.subtree_matches(expected_root, observed_root) {
                if self.exhausted {
                    return CompleteCausalAssignment::Exhausted;
                }
                continue;
            }
            let mut assignment = BTreeMap::new();
            if self.materialize(expected_root, observed_root, &mut assignment) {
                return CompleteCausalAssignment::Complete(assignment);
            }
            if self.exhausted {
                return CompleteCausalAssignment::Exhausted;
            }
        }
        CompleteCausalAssignment::Incomplete(self.remaining_steps)
    }

    fn subtree_matches(&mut self, expected_index: usize, observed_index: usize) -> bool {
        let identity = (expected_index, observed_index);
        if let Some(matches) = self.compatibility.get(&identity) {
            return *matches;
        }
        if !self.spend_step() {
            return false;
        }
        if !self.node_matches(expected_index, observed_index) {
            self.compatibility.insert(identity, false);
            return false;
        }

        let Some(expected_node) = self.expected_nodes.get(expected_index) else {
            return false;
        };
        let Some(observed_node) = self.observed_nodes.get(observed_index) else {
            return false;
        };
        let expected_key = expected_node.key().to_owned();
        let observed_id = observed_node.message_id().to_owned();
        let expected_children = self
            .expected_children
            .get(&expected_key)
            .cloned()
            .unwrap_or_default();
        let observed_children = self
            .observed_children
            .get(&observed_id)
            .cloned()
            .unwrap_or_default();
        let matches = self
            .sibling_assignment(&expected_children, &observed_children)
            .is_some();
        self.compatibility.insert(identity, matches);
        matches
    }

    fn node_matches(&self, expected_index: usize, observed_index: usize) -> bool {
        let Some(expected) = self.expected_nodes.get(expected_index) else {
            return false;
        };
        let Some(observed) = self.observed_nodes.get(observed_index) else {
            return false;
        };
        expected.kind() == observed.kind()
            && expected.name() == observed.name()
            && expected.schema_version() == observed.schema_version()
            && !aggregate_mismatch(expected, observed)
            && !payload_mismatch(expected, observed)
            && !outcome_mismatch(expected, observed, self.observed)
    }

    fn sibling_assignment(
        &mut self,
        expected: &[usize],
        observed: &[usize],
    ) -> Option<Vec<(usize, usize)>> {
        if expected.len() > observed.len() {
            return None;
        }
        let mut matched_by_observed = HashMap::<usize, usize>::new();
        // Later keys are inserted first so augmenting paths leave the earliest
        // expected key with the earliest compatible content/message identity.
        for expected_index in expected.iter().rev() {
            let mut visited = HashSet::new();
            if !self.augment_sibling(
                *expected_index,
                observed,
                &mut matched_by_observed,
                &mut visited,
            ) {
                return None;
            }
        }
        let mut pairs = matched_by_observed
            .into_iter()
            .map(|(observed, expected)| (expected, observed))
            .collect::<Vec<_>>();
        pairs.sort_by(|left, right| {
            match (
                self.expected_nodes.get(left.0),
                self.expected_nodes.get(right.0),
            ) {
                (Some(left), Some(right)) => left.key().cmp(right.key()),
                _ => left.cmp(right),
            }
        });
        Some(pairs)
    }

    fn augment_sibling(
        &mut self,
        expected_index: usize,
        observed: &[usize],
        matched_by_observed: &mut HashMap<usize, usize>,
        visited: &mut HashSet<usize>,
    ) -> bool {
        for observed_index in observed {
            if !self.spend_step() {
                return false;
            }
            if !self.subtree_matches(expected_index, *observed_index)
                || !visited.insert(*observed_index)
            {
                continue;
            }
            let can_assign =
                matched_by_observed
                    .get(observed_index)
                    .copied()
                    .is_none_or(|previous_expected| {
                        self.augment_sibling(
                            previous_expected,
                            observed,
                            matched_by_observed,
                            visited,
                        )
                    });
            if can_assign {
                matched_by_observed.insert(*observed_index, expected_index);
                return true;
            }
        }
        false
    }

    const fn spend_step(&mut self) -> bool {
        if !spend_comparison_step(&mut self.remaining_steps) {
            self.exhausted = true;
            return false;
        }
        true
    }

    fn materialize(
        &mut self,
        expected_index: usize,
        observed_index: usize,
        assignment: &mut BTreeMap<String, String>,
    ) -> bool {
        let Some(expected_node) = self.expected_nodes.get(expected_index) else {
            return false;
        };
        let Some(observed_node) = self.observed_nodes.get(observed_index) else {
            return false;
        };
        let expected_key = expected_node.key().to_owned();
        let observed_id = observed_node.message_id().to_owned();
        assignment.insert(expected_key.clone(), observed_id.clone());
        let expected_children = self
            .expected_children
            .get(&expected_key)
            .cloned()
            .unwrap_or_default();
        let observed_children = self
            .observed_children
            .get(&observed_id)
            .cloned()
            .unwrap_or_default();
        let Some(pairs) = self.sibling_assignment(&expected_children, &observed_children) else {
            return false;
        };
        pairs
            .into_iter()
            .all(|(expected, observed)| self.materialize(expected, observed, assignment))
    }
}

fn candidate_rank(
    expected: &ExpectedMessageNode,
    observed: &ObservedMessageNode,
    series: &ObservedMessageSeries,
) -> (bool, bool, bool, bool, bool, bool, String, String) {
    (
        expected.kind() != observed.kind(),
        expected.name() != observed.name(),
        expected.schema_version() != observed.schema_version(),
        aggregate_mismatch(expected, observed),
        payload_mismatch(expected, observed),
        outcome_mismatch(expected, observed, series),
        candidate_content_key(observed, series),
        observed.message_id().to_owned(),
    )
}

fn candidate_content_key(observed: &ObservedMessageNode, series: &ObservedMessageSeries) -> String {
    serde_json::to_string(&serde_json::json!({
        "kind": observed.kind(),
        "name": observed.name(),
        "schemaVersion": observed.schema_version(),
        "aggregate": observed.aggregate(),
        "payload": observed.payload(),
        "outcome": series
            .command_outcome(observed.message_id())
            .map(ObservedCommandOutcome::outcome),
    }))
    .unwrap_or_default()
}

fn aggregate_mismatch(expected: &ExpectedMessageNode, observed: &ObservedMessageNode) -> bool {
    expected
        .aggregate()
        .is_some_and(|aggregate| observed.aggregate() != Some(aggregate))
}

fn payload_mismatch(expected: &ExpectedMessageNode, observed: &ObservedMessageNode) -> bool {
    expected.payload().is_some_and(|expected_payload| {
        observed
            .payload()
            .is_none_or(|payload| !crate::payload_matches_subset(expected_payload, payload))
    })
}

fn outcome_mismatch(
    expected: &ExpectedMessageNode,
    observed: &ObservedMessageNode,
    series: &ObservedMessageSeries,
) -> bool {
    let Some(expected_outcome) = expected.outcome() else {
        return false;
    };
    let Some(observed_outcome) = series.command_outcome(observed.message_id()) else {
        return true;
    };
    !command_outcomes_match(expected_outcome, observed_outcome.outcome())
}

fn command_outcomes_match(expected: &TestOutcome, observed: &CommandResponseOutcome) -> bool {
    match (expected, observed) {
        (TestOutcome::Accepted, CommandResponseOutcome::Accepted) => true,
        (TestOutcome::Rejected(expected), CommandResponseOutcome::Rejected(observed)) => {
            expected.code == observed.code().as_str()
                && expected.payload.as_ref().is_none_or(|payload| {
                    observed
                        .details()
                        .is_some_and(|details| crate::payload_matches_subset(payload, details))
                })
        }
        _ => false,
    }
}

fn diagnose_unmatched_expected(
    expected: &ExpectedMessageNode,
    observed: &ObservedMessageSeries,
    matched_ids: &HashSet<String>,
    expected_parent_id: Option<&str>,
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
    remaining_steps: &mut usize,
) -> bool {
    let mut content_matches = Vec::new();
    for candidate in observed.messages().iter() {
        if !spend_comparison_step(remaining_steps) {
            return false;
        }
        if expected.kind() == candidate.kind()
            && expected.name() == candidate.name()
            && expected.schema_version() == candidate.schema_version()
            && !aggregate_mismatch(expected, candidate)
            && !payload_mismatch(expected, candidate)
            && !outcome_mismatch(expected, candidate, observed)
        {
            content_matches.push(candidate);
        }
    }
    content_matches.sort_by_key(|candidate| candidate.message_id());
    if let Some(candidate) = content_matches
        .iter()
        .copied()
        .find(|candidate| !matched_ids.contains(candidate.message_id()))
    {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "causation-mismatch",
            format!("expected:{}/parentKey", expected.key()),
            format!(
                "expected message `{}` has a content match with the wrong observed causation identity",
                expected.key()
            ),
            Some(expected_parent_id.map_or(Value::Null, Value::from)),
            Some(candidate.causation_id().map_or(Value::Null, Value::from)),
        ));
    } else if let Some(candidate) = content_matches.first() {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "identity-conflict",
            format!("expected:{}", expected.key()),
            "one observed identity would have to satisfy multiple expected nodes",
            Some(json_value(expected)),
            Some(Value::from(candidate.message_id())),
        ));
    }
    diagnostics.push(MessageSeriesComparisonDiagnostic::new(
        "missing-expected",
        format!("expected:{}", expected.key()),
        format!(
            "no observed message satisfies expected node `{}`",
            expected.key()
        ),
        Some(json_value(expected)),
        None,
    ));
    true
}

const fn spend_comparison_step(remaining_steps: &mut usize) -> bool {
    let Some(remaining) = remaining_steps.checked_sub(1) else {
        return false;
    };
    *remaining_steps = remaining;
    true
}

fn diagnose_matched_fields(
    expected: &ExpectedMessageNode,
    observed: &ObservedMessageNode,
    series: &ObservedMessageSeries,
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
) {
    let base = format!("expected:{}", expected.key());
    push_field_mismatch(
        expected.kind() != observed.kind(),
        "kind-mismatch",
        &format!("{base}/kind"),
        "message kind differs",
        json_value(&expected.kind()),
        json_value(&observed.kind()),
        diagnostics,
    );
    push_field_mismatch(
        expected.name() != observed.name(),
        "name-mismatch",
        &format!("{base}/name"),
        "message name differs",
        Value::from(expected.name()),
        Value::from(observed.name()),
        diagnostics,
    );
    push_field_mismatch(
        expected.schema_version() != observed.schema_version(),
        "schema-version-mismatch",
        &format!("{base}/schemaVersion"),
        "message schema version differs",
        Value::from(expected.schema_version()),
        Value::from(observed.schema_version()),
        diagnostics,
    );
    if aggregate_mismatch(expected, observed) {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "aggregate-mismatch",
            format!("{base}/aggregate"),
            "command aggregate identity differs",
            expected.aggregate().map(json_value),
            observed.aggregate().map(json_value),
        ));
    }
    if payload_mismatch(expected, observed) {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "payload-mismatch",
            format!("{base}/payload"),
            "observed payload does not contain the expected JSON subset",
            expected.payload().cloned(),
            observed.payload().cloned(),
        ));
    }
    if outcome_mismatch(expected, observed, series) {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "command-outcome-mismatch",
            format!("{base}/outcome"),
            "observed command outcome does not match the expectation",
            expected.outcome().map(json_value),
            series
                .command_outcome(observed.message_id())
                .map(|outcome| json_value(outcome.outcome())),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn push_field_mismatch(
    mismatch: bool,
    code: &'static str,
    path: &str,
    message: &str,
    expected: Value,
    observed: Value,
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
) {
    if mismatch {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            code,
            path,
            message,
            Some(expected),
            Some(observed),
        ));
    }
}

fn diagnose_unexpected_observations(
    observed: &ObservedMessageSeries,
    matched_ids: &HashSet<String>,
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
) {
    let mut unexpected = observed
        .messages()
        .iter()
        .filter(|message| !matched_ids.contains(message.message_id()))
        .collect::<Vec<_>>();
    unexpected.sort_by_key(|message| message.message_id());
    for message in unexpected {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "unexpected-observed",
            format!("observed:{}", message.message_id()),
            format!(
                "observed message `{}` was not expected",
                message.message_id()
            ),
            None,
            Some(json_value(message)),
        ));
    }
}

fn observed_structure_diagnostics(
    observed: &ObservedMessageSeries,
) -> Vec<MessageSeriesComparisonDiagnostic> {
    let mut diagnostics = Vec::new();
    for issue in observed.topology_issues() {
        match issue {
            MessageSeriesTopologyIssue::UnresolvedParent { child } => {
                diagnostics.push(MessageSeriesComparisonDiagnostic::new(
                    "unresolved-observed-parent",
                    format!("observed:{}/causationId", child.message_id()),
                    "observed causation identity does not resolve to a message",
                    None,
                    child.causation_id().map(Value::from),
                ));
            }
            MessageSeriesTopologyIssue::CrossCorrelation { child, parent } => {
                diagnostics.push(MessageSeriesComparisonDiagnostic::new(
                    "cross-correlation-link",
                    format!("observed:{}/correlationId", child.message_id()),
                    "observed child and parent have different correlation identities",
                    Some(Value::from(parent.correlation_id())),
                    Some(Value::from(child.correlation_id())),
                ));
            }
            MessageSeriesTopologyIssue::Cycle { nodes } => {
                let mut ids = nodes
                    .into_iter()
                    .map(ObservedMessageNode::message_id)
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                diagnostics.push(MessageSeriesComparisonDiagnostic::new(
                    "causation-cycle",
                    format!("observed:{}", ids.first().copied().unwrap_or("cycle")),
                    "observed causation links contain a cycle",
                    None,
                    Some(json_value(&ids)),
                ));
            }
            _ => {}
        }
    }
    for issue in observed.outcome_issues() {
        match issue {
            ObservedMessageSeriesOutcomeIssue::MissingCommand { outcome } => {
                diagnostics.push(MessageSeriesComparisonDiagnostic::new(
                    "unresolved-command-outcome",
                    format!(
                        "observed-outcome:{}/commandMessageId",
                        outcome.response_message_id()
                    ),
                    "command outcome references an unobserved command identity",
                    None,
                    Some(Value::from(outcome.command_message_id())),
                ));
            }
            ObservedMessageSeriesOutcomeIssue::NotACommand { outcome, message } => diagnostics
                .push(MessageSeriesComparisonDiagnostic::new(
                    "identity-conflict",
                    format!(
                        "observed-outcome:{}/commandMessageId",
                        outcome.response_message_id()
                    ),
                    "command outcome identity resolves to a non-command message",
                    Some(Value::from("command")),
                    Some(json_value(&message.kind())),
                )),
            ObservedMessageSeriesOutcomeIssue::CrossCorrelation { outcome, message } => diagnostics
                .push(MessageSeriesComparisonDiagnostic::new(
                    "cross-correlation-link",
                    format!(
                        "observed-outcome:{}/correlationId",
                        outcome.response_message_id()
                    ),
                    "command outcome and command have different correlation identities",
                    Some(Value::from(message.correlation_id())),
                    Some(Value::from(outcome.correlation_id())),
                )),
        }
    }
    diagnostics
}

fn diagnose_timing(
    observed: &ObservedMessageSeries,
    context: MessageSeriesComparisonContext,
    missing: &[String],
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
) {
    if context.timed_out && !missing.is_empty() {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            "timeout-before-expectations",
            "/expected",
            "the comparison deadline elapsed before all expectations matched",
            Some(json_value(missing)),
            context.timeout_observation_order.map(Value::from),
        ));
    }
    for message in observed.messages().iter() {
        diagnose_observation_timing(
            TimedObservationKind::Message,
            message.message_id(),
            message.observation_order(),
            context,
            diagnostics,
        );
    }
    for outcome in observed.command_outcomes() {
        diagnose_observation_timing(
            TimedObservationKind::CommandOutcome,
            outcome.response_message_id(),
            outcome.observation_order(),
            context,
            diagnostics,
        );
    }
}

#[derive(Clone, Copy)]
enum TimedObservationKind {
    Message,
    CommandOutcome,
}

fn diagnose_observation_timing(
    kind: TimedObservationKind,
    id: &str,
    order: u64,
    context: MessageSeriesComparisonContext,
    diagnostics: &mut Vec<MessageSeriesComparisonDiagnostic>,
) {
    let (path, after_timeout_code, after_timeout_message) = match kind {
        TimedObservationKind::Message => (
            format!("observed:{id}/observationOrder"),
            "message-after-timeout",
            "message arrived after the comparison deadline",
        ),
        TimedObservationKind::CommandOutcome => (
            format!("observed-outcome:{id}/observationOrder"),
            "command-outcome-after-timeout",
            "command outcome arrived after the comparison deadline",
        ),
    };
    if context
        .timeout_observation_order
        .is_some_and(|boundary| order >= boundary)
    {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            after_timeout_code,
            &path,
            after_timeout_message,
            None,
            Some(Value::from(order)),
        ));
    }

    let timing = context
        .settle_completed_at_order
        .filter(|boundary| order >= *boundary)
        .map(|_| match kind {
            TimedObservationKind::Message => (
                "message-after-settle",
                "message arrived after settling completed",
            ),
            TimedObservationKind::CommandOutcome => (
                "command-outcome-after-settle",
                "command outcome arrived after settling completed",
            ),
        })
        .or_else(|| {
            context
                .settle_started_at_order
                .filter(|boundary| order >= *boundary)
                .map(|_| match kind {
                    TimedObservationKind::Message => {
                        ("message-during-settle", "message arrived during settling")
                    }
                    TimedObservationKind::CommandOutcome => (
                        "command-outcome-during-settle",
                        "command outcome arrived during settling",
                    ),
                })
        });
    if let Some((code, message)) = timing {
        diagnostics.push(MessageSeriesComparisonDiagnostic::new(
            code,
            path,
            message,
            None,
            Some(Value::from(order)),
        ));
    }
}

fn json_value(value: &(impl Serialize + ?Sized)) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub fn message_series_definition_schema() -> Schema {
    let mut schema = schema_for!(MessageSeriesDefinition);
    add_unsigned_integer_maxima(&mut schema);
    schema
}

pub fn observed_message_series_schema() -> Schema {
    let mut schema = schema_for!(ObservedMessageSeries);
    add_unsigned_integer_maxima(&mut schema);
    schema
}

pub fn add_unsigned_integer_maxima(schema: &mut Schema) {
    if let Some(object) = schema.as_object_mut() {
        add_unsigned_integer_maxima_to_object(object);
    }
}

fn add_unsigned_integer_maxima_to_value(value: &mut Value) {
    match value {
        Value::Object(object) => add_unsigned_integer_maxima_to_object(object),
        Value::Array(values) => {
            for value in values {
                add_unsigned_integer_maxima_to_value(value);
            }
        }
        _ => {}
    }
}

fn add_unsigned_integer_maxima_to_object(object: &mut serde_json::Map<String, Value>) {
    let maximum = match object.get("format").and_then(Value::as_str) {
        Some("uint32") => Some(Value::from(u32::MAX)),
        Some("uint64") => Some(Value::from(u64::MAX)),
        _ => None,
    };
    if let Some(maximum) = maximum {
        object.entry("maximum").or_insert(maximum);
    }
    for value in object.values_mut() {
        add_unsigned_integer_maxima_to_value(value);
    }
}

fn validate_graph(nodes: &[ExpectedMessageNode], path: &str) -> Vec<MessageSeriesValidationIssue> {
    let nodes_path = graph_path(path, "nodes");
    if nodes.is_empty() {
        return vec![MessageSeriesValidationIssue::new(
            "empty-message-graph",
            nodes_path,
            "message graph must contain at least one node",
        )];
    }

    let mut issues = Vec::new();
    if nodes.len() > MAX_MESSAGE_SERIES_NODES {
        issues.push(MessageSeriesValidationIssue::new(
            "too-many-nodes",
            &nodes_path,
            format!("message graph contains more than {MAX_MESSAGE_SERIES_NODES} nodes"),
        ));
    }

    let (first_by_key, duplicate_keys) = validate_nodes(nodes, &nodes_path, &mut issues);
    validate_parent_keys(nodes, &nodes_path, &first_by_key, &mut issues);
    validate_root(nodes, &nodes_path, &mut issues);
    validate_cycles(
        nodes,
        &nodes_path,
        &first_by_key,
        duplicate_keys,
        &mut issues,
    );
    validate_comparison_bounds(
        nodes,
        &nodes_path,
        &first_by_key,
        duplicate_keys,
        &mut issues,
    );
    issues
}

fn validate_comparison_bounds(
    nodes: &[ExpectedMessageNode],
    nodes_path: &str,
    first_by_key: &HashMap<&str, usize>,
    duplicate_keys: bool,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) {
    let mut children_by_parent = HashMap::<&str, usize>::new();
    for node in nodes {
        if let Some(parent_key) = node.parent_key() {
            let count = children_by_parent.entry(parent_key).or_default();
            *count = count.saturating_add(1);
        }
    }
    if let Some((parent, count)) = children_by_parent
        .into_iter()
        .find(|(_, count)| *count > MAXIMUM_EXPECTED_SIBLINGS)
    {
        issues.push(MessageSeriesValidationIssue::new(
            "too-many-siblings",
            nodes_path,
            format!(
                "node `{parent}` has {count} children; at most {MAXIMUM_EXPECTED_SIBLINGS} are supported"
            ),
        ));
    }

    if duplicate_keys {
        return;
    }
    for node in nodes {
        let mut visited = HashSet::new();
        let mut depth = 0_usize;
        let mut parent = node.parent_key();
        while let Some(parent_key) = parent {
            if !visited.insert(parent_key) {
                break;
            }
            depth = depth.saturating_add(1);
            if depth > MAXIMUM_EXPECTED_GRAPH_DEPTH {
                issues.push(MessageSeriesValidationIssue::new(
                    "message-graph-too-deep",
                    nodes_path,
                    format!(
                        "message graph depth exceeds the supported maximum of {MAXIMUM_EXPECTED_GRAPH_DEPTH}"
                    ),
                ));
                return;
            }
            parent = first_by_key
                .get(parent_key)
                .and_then(|index| nodes.get(*index))
                .and_then(ExpectedMessageNode::parent_key);
        }
    }
}

fn validate_nodes<'a>(
    nodes: &'a [ExpectedMessageNode],
    nodes_path: &str,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) -> (HashMap<&'a str, usize>, bool) {
    let mut first_by_key = HashMap::new();
    let mut duplicate_keys = false;
    for (index, node) in nodes.iter().enumerate() {
        let node_path = format!("{nodes_path}/{index}");
        if node.key().trim().is_empty() {
            issues.push(MessageSeriesValidationIssue::new(
                "empty-node-key",
                format!("{node_path}/key"),
                "node key must not be empty",
            ));
        }
        match first_by_key.entry(node.key()) {
            Entry::Vacant(entry) => {
                entry.insert(index);
            }
            Entry::Occupied(entry) => {
                duplicate_keys = true;
                issues.push(MessageSeriesValidationIssue::new(
                    "duplicate-node-key",
                    format!("{node_path}/key"),
                    format!(
                        "node key `{}` duplicates {nodes_path}/{}/key",
                        node.key(),
                        entry.get()
                    ),
                ));
            }
        }
        validate_node_fields(node, &node_path, issues);
    }
    (first_by_key, duplicate_keys)
}

fn validate_node_fields(
    node: &ExpectedMessageNode,
    node_path: &str,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) {
    if node
        .parent_key()
        .is_some_and(|parent| parent.trim().is_empty())
    {
        issues.push(MessageSeriesValidationIssue::new(
            "empty-parent-key",
            format!("{node_path}/parentKey"),
            "parent key must not be empty",
        ));
    }
    if node.name().trim().is_empty() {
        issues.push(MessageSeriesValidationIssue::new(
            "empty-message-name",
            format!("{node_path}/name"),
            "message name must not be empty",
        ));
    }
    if node.schema_version() == 0 {
        issues.push(MessageSeriesValidationIssue::new(
            "invalid-schema-version",
            format!("{node_path}/schemaVersion"),
            "message schema version must be greater than zero",
        ));
    }
    if let ExpectedMessageNode::Command {
        aggregate, outcome, ..
    } = node
    {
        if aggregate.aggregate_type.trim().is_empty() {
            issues.push(MessageSeriesValidationIssue::new(
                "empty-aggregate-type",
                format!("{node_path}/aggregate/type"),
                "aggregate type must not be empty",
            ));
        }
        if aggregate.id.trim().is_empty() {
            issues.push(MessageSeriesValidationIssue::new(
                "empty-aggregate-id",
                format!("{node_path}/aggregate/id"),
                "aggregate ID must not be empty",
            ));
        }
        if outcome
            .rejection()
            .is_some_and(|rejection| rejection.code.trim().is_empty())
        {
            issues.push(MessageSeriesValidationIssue::new(
                "empty-rejection-code",
                format!("{node_path}/outcome/rejected/code"),
                "rejection code must not be empty",
            ));
        }
    }
}

fn validate_parent_keys(
    nodes: &[ExpectedMessageNode],
    nodes_path: &str,
    first_by_key: &HashMap<&str, usize>,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) {
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent_key) = node.parent_key()
            && !parent_key.trim().is_empty()
            && !first_by_key.contains_key(parent_key)
        {
            issues.push(MessageSeriesValidationIssue::new(
                "unresolved-parent-key",
                format!("{nodes_path}/{index}/parentKey"),
                format!("parent key `{parent_key}` does not identify a node in this graph"),
            ));
        }
    }
}

fn validate_root(
    nodes: &[ExpectedMessageNode],
    nodes_path: &str,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) {
    let roots = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent_key().is_none())
        .collect::<Vec<_>>();
    if let Some((root_index, root)) = roots.first().copied().filter(|_| roots.len() == 1) {
        if !root.is_command() {
            issues.push(MessageSeriesValidationIssue::new(
                "root-not-command",
                format!("{nodes_path}/{root_index}/kind"),
                "message graph root must be a command",
            ));
        } else if matches!(root, ExpectedMessageNode::Command { payload: None, .. }) {
            issues.push(MessageSeriesValidationIssue::new(
                "missing-root-command-payload",
                format!("{nodes_path}/{root_index}/payload"),
                "root command must contain its complete input payload",
            ));
        }
    } else {
        issues.push(MessageSeriesValidationIssue::new(
            "invalid-root-count",
            nodes_path,
            format!(
                "message graph must contain exactly one root; found {}",
                roots.len()
            ),
        ));
    }
}

fn validate_cycles(
    nodes: &[ExpectedMessageNode],
    nodes_path: &str,
    first_by_key: &HashMap<&str, usize>,
    duplicate_keys: bool,
    issues: &mut Vec<MessageSeriesValidationIssue>,
) {
    if !duplicate_keys && nodes.len() <= MAX_MESSAGE_SERIES_NODES {
        let topology = MessageSeries::try_from_nodes(nodes.iter().map(|node| TopologyNode {
            key: node.key(),
            parent_key: node.parent_key(),
        }));
        if let Ok(topology) = topology {
            for issue in topology.topology_issues() {
                if let MessageSeriesTopologyIssue::Cycle { nodes: cycle } = issue {
                    let keys = cycle
                        .iter()
                        .map(|node| node.key)
                        .collect::<Vec<_>>()
                        .join(", ");
                    if let Some(first_index) =
                        cycle.first().and_then(|node| first_by_key.get(node.key))
                    {
                        issues.push(MessageSeriesValidationIssue::new(
                            "causation-cycle",
                            format!("{nodes_path}/{first_index}/parentKey"),
                            format!("causation cycle contains node keys: {keys}"),
                        ));
                    }
                }
            }
        }
    }
}

fn graph_path(base: &str, field: &str) -> String {
    if base.is_empty() {
        format!("/{field}")
    } else {
        format!("{base}/{field}")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TopologyNode<'a> {
    key: &'a str,
    parent_key: Option<&'a str>,
}

impl MessageSeriesNode for TopologyNode<'_> {
    type CorrelationId = ();
    type MessageId = str;

    fn message_id(&self) -> &Self::MessageId {
        self.key
    }

    fn correlation_id(&self) -> &Self::CorrelationId {
        &()
    }

    fn causation_id(&self) -> Option<&Self::MessageId> {
        self.parent_key
    }
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct TimeoutSchema(
    #[schemars(regex(
        pattern = r"^0*(?:(?:[1-9]|[1-9][0-9]{1,3}|[1-5][0-9]{4}|60000)ms|(?:[1-9]|[1-5][0-9]|60)s)$"
    ))]
    String,
);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct NonEmptyStringSchema(#[schemars(regex(pattern = r"\S"))] String);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(transparent)]
struct IdentifierSchema(
    #[schemars(length(min = 1, max = 256), regex(pattern = r"^[!-~]+$"))] String,
);

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
struct AggregateSchema {
    #[schemars(rename = "type", regex(pattern = r"\S"))]
    aggregate_type: String,
    #[schemars(regex(pattern = r"\S"))]
    id: String,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(untagged)]
enum ExpectedOutcomeSchema {
    Accepted(AcceptedOutcomeSchema),
    Rejected(RejectedOutcomeSchema),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "lowercase")]
enum AcceptedOutcomeSchema {
    Accepted,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
struct RejectedOutcomeSchema {
    rejected: ExpectedRejectionSchema,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
struct ExpectedRejectionSchema {
    #[schemars(regex(pattern = r"\S"))]
    code: String,
    payload: Option<Value>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(tag = "status", content = "value", rename_all = "snake_case")]
enum CommandResponseOutcomeSchema {
    #[schemars(transform = allow_null_accepted_outcome_value)]
    Accepted,
    Rejected(CommandRejectionSchema),
}

fn allow_null_accepted_outcome_value(schema: &mut Schema) {
    if let Some(properties) = schema
        .as_object_mut()
        .and_then(|schema| schema.get_mut("properties"))
        .and_then(Value::as_object_mut)
    {
        properties.insert("value".to_owned(), serde_json::json!({ "type": "null" }));
    }
}

#[allow(dead_code)]
#[derive(JsonSchema)]
struct CommandRejectionSchema {
    classification: CommandRejectionClassificationSchema,
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = r"^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$"),
        description = "A 1-128 byte ASCII application error code."
    )]
    code: String,
    #[schemars(
        length(min = 1),
        description = "A nonempty rejection message. Runtime validation additionally rejects Unicode control characters and values exceeding 1024 UTF-8 bytes; JSON Schema cannot express that byte limit."
    )]
    message: String,
    details: Option<Value>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
enum CommandRejectionClassificationSchema {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    Internal,
}

#[cfg(test)]
mod tests {
    use rostfrei_messaging_core::{
        ApplicationErrorCode, CommandRejection, CommandRejectionClassification,
        MessageSeriesInsertOutcome,
    };
    use serde_json::json;

    use super::*;
    use crate::TestRejection;

    fn timeout(value: &str) -> TestTimeout {
        value.parse().unwrap()
    }

    fn aggregate() -> TestAggregate {
        TestAggregate {
            aggregate_type: "bike-rental/rental-fleet".to_owned(),
            id: "city-fleet".to_owned(),
        }
    }

    fn expected_command(
        key: &str,
        parent_key: Option<&str>,
        payload: Option<Value>,
        outcome: TestOutcome,
    ) -> ExpectedMessageNode {
        ExpectedMessageNode::Command {
            key: key.to_owned(),
            parent_key: parent_key.map(str::to_owned),
            name: "rent-bicycle".to_owned(),
            schema_version: 1,
            aggregate: aggregate(),
            payload,
            outcome,
        }
    }

    fn expected_domain_event(key: &str, parent_key: Option<&str>) -> ExpectedMessageNode {
        ExpectedMessageNode::DomainEvent {
            key: key.to_owned(),
            parent_key: parent_key.map(str::to_owned),
            name: "bicycle-rented".to_owned(),
            schema_version: 1,
            payload: Some(json!({ "bicycle_id": "bike-42" })),
        }
    }

    fn observed_command(message_id: &str, correlation_id: &str) -> ObservedMessageNode {
        ObservedMessageNode::command(
            message_id,
            correlation_id,
            None,
            "rent-bicycle",
            1,
            aggregate(),
            Some(json!({ "bicycle_id": "bike-42" })),
        )
    }

    fn observed_domain_event(
        message_id: &str,
        correlation_id: &str,
        causation_id: Option<&str>,
    ) -> ObservedMessageNode {
        ObservedMessageNode::domain_event(
            message_id,
            correlation_id,
            causation_id.map(str::to_owned),
            "bicycle-rented",
            1,
            Some(aggregate()),
            Some(json!({ "bicycle_id": "bike-42" })),
        )
    }

    fn issue_codes(error: &MessageSeriesDefinitionError) -> Vec<&'static str> {
        error
            .issues()
            .iter()
            .map(MessageSeriesValidationIssue::code)
            .collect()
    }

    #[test]
    fn definitions_preserve_graph_order_parent_links_and_timing_inheritance() {
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_domain_event("rented", Some("rent")),
                expected_command(
                    "rent",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
            ],
            None,
            Some(timeout("1s")),
        )
        .unwrap();
        let definition = MessageSeriesDefinition::try_new(
            timeout("10s"),
            timeout("250ms"),
            vec![graph.clone(), graph],
        )
        .unwrap();

        assert_eq!(definition.graphs().len(), 2);
        assert_eq!(
            definition.graphs()[0]
                .nodes()
                .iter()
                .map(ExpectedMessageNode::key)
                .collect::<Vec<_>>(),
            ["rented", "rent"]
        );
        assert_eq!(
            definition.graphs()[0].effective_within(&definition),
            timeout("10s")
        );
        assert_eq!(
            definition.graphs()[0].effective_settle_for(&definition),
            timeout("1s")
        );

        let serialized = serde_json::to_value(&definition).unwrap();
        assert_eq!(
            serde_json::from_value::<MessageSeriesDefinition>(serialized).unwrap(),
            definition
        );
    }

    #[test]
    fn definition_outcomes_keep_the_existing_accepted_and_rejected_shapes() {
        let accepted = expected_command("accepted", None, Some(json!({})), TestOutcome::Accepted);
        let rejected = expected_command(
            "rejected",
            None,
            Some(json!({})),
            TestOutcome::Rejected(TestRejection {
                code: "BICYCLE_UNAVAILABLE".to_owned(),
                payload: Some(json!({ "bicycle_id": "bike-42" })),
            }),
        );

        assert_eq!(
            serde_json::to_value(accepted).unwrap()["outcome"],
            "accepted"
        );
        assert_eq!(
            serde_json::to_value(rejected).unwrap()["outcome"],
            json!({
                "rejected": {
                    "code": "BICYCLE_UNAVAILABLE",
                    "payload": { "bicycle_id": "bike-42" }
                }
            })
        );
    }

    #[test]
    fn definition_payloads_remain_literal_json_subsets() {
        let matcher_looking_payload = json!({
            "$capture": "bicycle",
            "nested": { "$pattern": "bike-.*" }
        });
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_command("root", None, Some(json!({})), TestOutcome::Accepted),
                ExpectedMessageNode::IntegrationEvent {
                    key: "published".to_owned(),
                    parent_key: Some("root".to_owned()),
                    name: "bicycle-rental-started".to_owned(),
                    schema_version: 1,
                    payload: Some(matcher_looking_payload.clone()),
                },
            ],
            None,
            None,
        )
        .unwrap();

        let serialized = serde_json::to_value(graph).unwrap();
        assert_eq!(serialized["nodes"][1]["payload"], matcher_looking_payload);
    }

    fn null_payload_definition_document() -> Value {
        json!({
            "within": "10s",
            "settleFor": "250ms",
            "graphs": [{
                "nodes": [{
                    "kind": "command",
                    "key": "subject",
                    "name": "rent-bicycle",
                    "schemaVersion": 1,
                    "aggregate": { "type": "bike-rental/rental-fleet", "id": "city-fleet" },
                    "payload": null,
                    "outcome": { "rejected": { "code": "BICYCLE_UNAVAILABLE", "payload": null } }
                }, {
                    "kind": "domain-event",
                    "key": "wildcard",
                    "parentKey": "subject",
                    "name": "wildcard-event",
                    "schemaVersion": 1
                }, {
                    "kind": "domain-event",
                    "key": "exact-null",
                    "parentKey": "subject",
                    "name": "null-event",
                    "schemaVersion": 1,
                    "payload": null
                }]
            }]
        })
    }

    #[test]
    fn explicit_null_payloads_survive_expected_wire_contracts() {
        let document = null_payload_definition_document();
        let definition = MessageSeriesDefinition::from_json_value(document.clone()).unwrap();
        let graph = &definition.graphs()[0];
        assert_eq!(graph.root_command().unwrap().payload, &Value::Null);
        assert_eq!(graph.nodes().get("wildcard").unwrap().payload(), None);
        assert_eq!(
            graph.nodes().get("exact-null").unwrap().payload(),
            Some(&Value::Null)
        );
        assert_eq!(
            graph
                .nodes()
                .get("subject")
                .unwrap()
                .outcome()
                .and_then(TestOutcome::rejection)
                .and_then(|rejection| rejection.payload.as_ref()),
            Some(&Value::Null)
        );
        let direct = serde_json::from_value::<ExpectedMessageNode>(
            document["graphs"][0]["nodes"][0].clone(),
        )
        .unwrap();
        assert_eq!(direct.payload(), Some(&Value::Null));
        assert_eq!(serde_json::to_value(&definition).unwrap(), document);
    }

    #[test]
    fn explicit_null_payloads_are_exact_while_absent_payloads_are_wildcards() {
        let definition =
            MessageSeriesDefinition::from_json_value(null_payload_definition_document()).unwrap();
        let graph = &definition.graphs()[0];
        let rejection = CommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("BICYCLE_UNAVAILABLE").unwrap(),
            "bicycle is unavailable",
            Some(Value::Null),
        )
        .unwrap();
        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(ObservedMessageNode::command(
                "command-1",
                "correlation-1",
                None,
                "rent-bicycle",
                1,
                aggregate(),
                Some(Value::Null),
            ))
            .unwrap();
        observed
            .insert_command_outcome(
                ObservedCommandOutcome::try_new(
                    "response-1",
                    "command-1",
                    "correlation-1",
                    CommandResponseOutcome::Rejected(rejection),
                )
                .unwrap(),
            )
            .unwrap();
        observed
            .insert_message(ObservedMessageNode::domain_event(
                "wildcard-1",
                "correlation-1",
                Some("command-1".to_owned()),
                "wildcard-event",
                1,
                None,
                Some(json!({ "anything": true })),
            ))
            .unwrap();
        observed
            .insert_message(ObservedMessageNode::domain_event(
                "null-1",
                "correlation-1",
                Some("command-1".to_owned()),
                "null-event",
                1,
                None,
                Some(Value::Null),
            ))
            .unwrap();

        let comparison =
            compare_message_series(graph, &observed, MessageSeriesComparisonContext::default());
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Passed);
        let serialized = serde_json::to_value(&observed).unwrap();
        assert_eq!(serialized["messages"][0]["payload"], Value::Null);
        assert_eq!(
            serde_json::from_value::<ObservedMessageNode>(serialized["messages"][0].clone())
                .unwrap()
                .payload(),
            Some(&Value::Null)
        );

        let expected_null = Value::Null;
        assert!(crate::payload_matches_subset(&expected_null, &Value::Null));
        assert!(!crate::payload_matches_subset(&expected_null, &json!({})));
        let rejection_without_details = CommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("BICYCLE_UNAVAILABLE").unwrap(),
            "bicycle is unavailable",
            None,
        )
        .unwrap();
        assert!(!command_outcomes_match(
            graph.nodes().get("subject").unwrap().outcome().unwrap(),
            &CommandResponseOutcome::Rejected(rejection_without_details)
        ));
        assert!(payload_mismatch(
            graph.nodes().get("exact-null").unwrap(),
            &ObservedMessageNode::domain_event(
                "missing-payload",
                "correlation-1",
                Some("command-1".to_owned()),
                "null-event",
                1,
                None,
                None,
            )
        ));
    }

    #[test]
    fn definition_validation_rejects_duplicate_and_unresolved_keys() {
        let error = MessageGraphDefinition::try_from_nodes(
            [
                expected_command("same", None, Some(json!({})), TestOutcome::Accepted),
                expected_domain_event("same", Some("missing")),
            ],
            None,
            None,
        )
        .unwrap_err();

        assert!(issue_codes(&error).contains(&"duplicate-node-key"));
        assert!(issue_codes(&error).contains(&"unresolved-parent-key"));
        let duplicate = error
            .issues()
            .iter()
            .find(|issue| issue.code() == "duplicate-node-key")
            .unwrap();
        assert_eq!(duplicate.path(), "/nodes/1/key");
        assert!(duplicate.message().contains("/nodes/0/key"));

        let identical = expected_command("duplicate", None, Some(json!({})), TestOutcome::Accepted);
        let error =
            MessageGraphDefinition::try_from_nodes([identical.clone(), identical], None, None)
                .unwrap_err();
        assert!(issue_codes(&error).contains(&"duplicate-node-key"));
    }

    #[test]
    fn definition_validation_rejects_invalid_roots_and_cycles() {
        let cycle = MessageGraphDefinition::try_from_nodes(
            [
                expected_command(
                    "first",
                    Some("second"),
                    Some(json!({})),
                    TestOutcome::Accepted,
                ),
                expected_domain_event("second", Some("first")),
            ],
            None,
            None,
        )
        .unwrap_err();
        assert!(issue_codes(&cycle).contains(&"invalid-root-count"));
        assert!(issue_codes(&cycle).contains(&"causation-cycle"));

        let event_root = MessageGraphDefinition::try_from_nodes(
            [expected_domain_event("event", None)],
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(issue_codes(&event_root), ["root-not-command"]);

        let missing_payload = MessageGraphDefinition::try_from_nodes(
            [expected_command("root", None, None, TestOutcome::Accepted)],
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(
            issue_codes(&missing_payload),
            ["missing-root-command-payload"]
        );
    }

    #[test]
    fn definition_validation_bounds_comparator_depth_and_sibling_search() {
        let mut deep = vec![expected_command(
            "root",
            None,
            Some(json!({})),
            TestOutcome::Accepted,
        )];
        let mut parent = "root".to_owned();
        for depth in 1..=MAXIMUM_EXPECTED_GRAPH_DEPTH + 1 {
            let key = format!("event-{depth}");
            deep.push(expected_domain_event(&key, Some(&parent)));
            parent = key;
        }
        let error = MessageGraphDefinition::try_from_nodes(deep, None, None).unwrap_err();
        assert!(issue_codes(&error).contains(&"message-graph-too-deep"));

        let mut wide = vec![expected_command(
            "root",
            None,
            Some(json!({})),
            TestOutcome::Accepted,
        )];
        for index in 0..=MAXIMUM_EXPECTED_SIBLINGS {
            wide.push(expected_domain_event(
                &format!("event-{index}"),
                Some("root"),
            ));
        }
        let error = MessageGraphDefinition::try_from_nodes(wide, None, None).unwrap_err();
        assert!(issue_codes(&error).contains(&"too-many-siblings"));
    }

    #[test]
    fn definition_deserialization_rejects_unknown_fields_and_kinds() {
        let valid = json!({
            "within": "10s",
            "settleFor": "250ms",
            "graphs": [{
                "nodes": [{
                    "kind": "command",
                    "key": "root",
                    "name": "rent-bicycle",
                    "schemaVersion": 1,
                    "aggregate": { "type": "bike-rental/rental-fleet", "id": "city-fleet" },
                    "payload": {},
                    "outcome": "accepted"
                }]
            }]
        });
        let mut unknown_field = valid.clone();
        unknown_field["graphs"][0]["nodes"][0]["unknown"] = json!(true);
        assert!(serde_json::from_value::<MessageSeriesDefinition>(unknown_field).is_err());

        let mut unknown_kind = valid;
        unknown_kind["graphs"][0]["nodes"][0]["kind"] = json!("query");
        assert!(serde_json::from_value::<MessageSeriesDefinition>(unknown_kind).is_err());
    }

    #[test]
    fn definition_json_validation_preserves_structured_semantic_issues() {
        let document = json!({
            "within": "10s",
            "settleFor": "250ms",
            "graphs": [{
                "nodes": [{
                    "kind": "command",
                    "key": "root",
                    "parentKey": "missing",
                    "name": "rent-bicycle",
                    "schemaVersion": 1,
                    "aggregate": { "type": "bike-rental/rental-fleet", "id": "city-fleet" },
                    "payload": {},
                    "outcome": "accepted"
                }]
            }]
        });

        let error = MessageSeriesDefinition::from_json_value(document.clone()).unwrap_err();
        let unresolved = error
            .issues()
            .iter()
            .find(|issue| issue.code() == "unresolved-parent-key")
            .unwrap();
        assert_eq!(unresolved.path(), "/graphs/0/nodes/0/parentKey");

        let mut empty_contract_values = document.clone();
        empty_contract_values["graphs"][0]["nodes"][0]["aggregate"]["type"] = json!("");
        empty_contract_values["graphs"][0]["nodes"][0]["outcome"] = json!({
            "rejected": { "code": "" }
        });
        let error = MessageSeriesDefinition::from_json_value(empty_contract_values).unwrap_err();
        assert!(issue_codes(&error).contains(&"empty-aggregate-type"));
        assert!(issue_codes(&error).contains(&"empty-rejection-code"));

        let mut invalid_outcome = document;
        invalid_outcome["graphs"][0]["nodes"][0]["outcome"] = json!("unknown");
        let error = MessageSeriesDefinition::from_json_value(invalid_outcome).unwrap_err();
        assert!(issue_codes(&error).contains(&"invalid-outcome"));

        let malformed = MessageSeriesDefinition::from_json_value(json!({})).unwrap_err();
        assert_eq!(issue_codes(&malformed), ["invalid-message-series-document"]);
    }

    #[test]
    fn definition_validation_rejects_empty_series_and_graphs() {
        let series = MessageSeriesDefinition::try_new(timeout("10s"), timeout("250ms"), Vec::new())
            .unwrap_err();
        assert_eq!(issue_codes(&series), ["empty-message-series"]);
        assert_eq!(series.issues()[0].path(), "/graphs");

        let graph = MessageGraphDefinition::try_from_nodes([], None, None).unwrap_err();
        assert_eq!(issue_codes(&graph), ["empty-message-graph"]);
        assert_eq!(graph.issues()[0].path(), "/nodes");
    }

    #[test]
    fn observed_series_resolves_messages_and_independently_arriving_outcomes() {
        let mut series = ObservedMessageSeries::new();
        series
            .insert_message(observed_domain_event(
                "event-1",
                "correlation-1",
                Some("command-1"),
            ))
            .unwrap();
        let outcome = ObservedCommandOutcome::try_new(
            "response-1",
            "command-1",
            "correlation-1",
            CommandResponseOutcome::Accepted,
        )
        .unwrap();
        series.insert_command_outcome(outcome).unwrap();

        assert_eq!(series.messages().unresolved_nodes().count(), 1);
        let stored_outcome = series.command_outcomes().first().unwrap();
        assert_eq!(
            series.outcome_issues(),
            [ObservedMessageSeriesOutcomeIssue::MissingCommand {
                outcome: stored_outcome
            }]
        );

        series
            .insert_message(observed_command("command-1", "correlation-1"))
            .unwrap();

        assert!(series.topology_issues().is_empty());
        assert!(series.outcome_issues().is_empty());
        assert_eq!(
            series
                .command_outcome("command-1")
                .unwrap()
                .response_message_id(),
            "response-1"
        );

        let serialized = serde_json::to_value(&series).unwrap();
        assert!(serialized["messages"].is_array());
        assert!(serialized["commandOutcomes"].is_array());
        assert_eq!(
            serde_json::from_value::<ObservedMessageSeries>(serialized).unwrap(),
            series
        );
    }

    #[test]
    fn observed_outcomes_are_idempotent_and_report_bad_attachments() {
        let command = observed_command("command-1", "correlation-1");
        let outcome = ObservedCommandOutcome::try_new(
            "response-1",
            "command-1",
            "correlation-2",
            CommandResponseOutcome::Accepted,
        )
        .unwrap();
        let mut series =
            ObservedMessageSeries::try_from_parts([command], [outcome.clone()]).unwrap();

        assert_eq!(
            series.insert_command_outcome(outcome).unwrap(),
            MessageSeriesInsertOutcome::Duplicate
        );
        assert!(matches!(
            series.outcome_issues().as_slice(),
            [ObservedMessageSeriesOutcomeIssue::CrossCorrelation { .. }]
        ));

        let conflict = ObservedCommandOutcome::try_new(
            "response-2",
            "command-1",
            "correlation-2",
            CommandResponseOutcome::Accepted,
        )
        .unwrap();
        let error = series.insert_command_outcome(conflict).unwrap_err();
        assert_eq!(error, ObservedMessageSeriesError::OutcomeIdentityConflict);

        let response_conflict = ObservedCommandOutcome::try_new(
            "response-1",
            "command-2",
            "correlation-2",
            CommandResponseOutcome::Accepted,
        )
        .unwrap();
        let error = series
            .insert_command_outcome(response_conflict)
            .unwrap_err();
        assert_eq!(error, ObservedMessageSeriesError::OutcomeIdentityConflict);
    }

    #[test]
    fn observed_series_reports_non_command_outcomes_and_message_topology() {
        let event = observed_domain_event("event-1", "correlation-1", Some("command-1"));
        let wrong_outcome = ObservedCommandOutcome::try_new(
            "response-1",
            "event-1",
            "correlation-1",
            CommandResponseOutcome::Accepted,
        )
        .unwrap();
        let series = ObservedMessageSeries::try_from_parts([event], [wrong_outcome]).unwrap();

        assert!(matches!(
            series.outcome_issues().as_slice(),
            [ObservedMessageSeriesOutcomeIssue::NotACommand { .. }]
        ));
        assert!(matches!(
            series.topology_issues().as_slice(),
            [MessageSeriesTopologyIssue::UnresolvedParent { .. }]
        ));
    }

    #[test]
    fn observed_deserialization_rejects_unknown_fields_and_kinds() {
        let mut unknown_field = serde_json::to_value(ObservedMessageSeries::new()).unwrap();
        unknown_field["unknown"] = json!(true);
        assert!(serde_json::from_value::<ObservedMessageSeries>(unknown_field).is_err());

        let unknown_kind = json!({
            "messages": [{
                "kind": "query",
                "messageId": "query-1",
                "correlationId": "correlation-1",
                "observationOrder": 0,
                "name": "find-bicycle",
                "schemaVersion": 1
            }],
            "commandOutcomes": []
        });
        assert!(serde_json::from_value::<ObservedMessageSeries>(unknown_kind).is_err());

        let mut accepted_with_value = json!({
            "messages": [],
            "commandOutcomes": [{
                "responseMessageId": "response-1",
                "commandMessageId": "command-1",
                "correlationId": "correlation-1",
                "observationOrder": 0,
                "outcome": { "status": "accepted", "value": {} }
            }]
        });
        assert!(
            serde_json::from_value::<ObservedMessageSeries>(accepted_with_value.clone()).is_err()
        );
        accepted_with_value["commandOutcomes"][0]["outcome"]["value"] = Value::Null;
        assert!(serde_json::from_value::<ObservedMessageSeries>(accepted_with_value).is_ok());
    }

    #[test]
    fn observed_deserialization_rejects_identical_duplicate_history_entries() {
        let mut series = ObservedMessageSeries::new();
        series
            .insert_message(observed_command("command-1", "correlation-1"))
            .unwrap();
        let message = serde_json::to_value(&series).unwrap()["messages"][0].clone();
        let duplicate_messages = json!({
            "messages": [message.clone(), message],
            "commandOutcomes": []
        });
        let error = serde_json::from_value::<ObservedMessageSeries>(duplicate_messages)
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate message identity"));

        let outcome = accepted_outcome("command-1", "correlation-1");
        let outcome = serde_json::to_value(outcome).unwrap();
        let duplicate_outcomes = json!({
            "messages": [],
            "commandOutcomes": [outcome.clone(), outcome]
        });
        let error = serde_json::from_value::<ObservedMessageSeries>(duplicate_outcomes)
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate command outcome"));
    }

    #[test]
    fn observed_contract_rejects_invalid_values_and_raw_array_overflow() {
        let invalid_message = ObservedMessageNode::DomainEvent {
            message_id: String::new(),
            correlation_id: "correlation-1".to_owned(),
            causation_id: None,
            observation_order: 0,
            name: "bicycle-rented".to_owned(),
            schema_version: 1,
            aggregate: None,
            payload: None,
        };
        assert_eq!(
            ObservedMessageSeries::try_from_parts([invalid_message], []),
            Err(ObservedMessageSeriesError::InvalidMessage { field: "messageId" })
        );
        assert_eq!(
            ObservedCommandOutcome::try_new(
                "",
                "command-1",
                "correlation-1",
                CommandResponseOutcome::Accepted,
            ),
            Err(ObservedMessageSeriesError::InvalidOutcome {
                field: "responseMessageId"
            })
        );

        let invalid_schema_version = json!({
            "messages": [{
                "kind": "domain-event",
                "messageId": "event-1",
                "correlationId": "correlation-1",
                "observationOrder": 0,
                "name": "bicycle-rented",
                "schemaVersion": 0
            }],
            "commandOutcomes": []
        });
        assert!(serde_json::from_value::<ObservedMessageSeries>(invalid_schema_version).is_err());

        let repeated = observed_command("command-1", "correlation-1");
        let oversized = json!({
            "messages": vec![repeated; MAX_MESSAGE_SERIES_NODES + 1],
            "commandOutcomes": []
        });
        assert!(serde_json::from_value::<ObservedMessageSeries>(oversized).is_err());
    }

    fn accepted_outcome(command_id: &str, correlation_id: &str) -> ObservedCommandOutcome {
        ObservedCommandOutcome::try_new(
            format!("response-{command_id}"),
            command_id,
            correlation_id,
            CommandResponseOutcome::Accepted,
        )
        .unwrap()
    }

    fn diagnostic_codes(comparison: &MessageSeriesComparison) -> Vec<&'static str> {
        comparison
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn observation_orders_are_global_automatic_and_round_trip() {
        let mut series = ObservedMessageSeries::new();
        assert_eq!(
            series
                .insert_message(observed_command("command-1", "correlation-1"))
                .unwrap(),
            MessageSeriesInsertOutcome::Inserted
        );
        assert_eq!(
            series
                .insert_message(observed_command("command-1", "correlation-1"))
                .unwrap(),
            MessageSeriesInsertOutcome::Duplicate
        );
        series
            .insert_command_outcome(accepted_outcome("command-1", "correlation-1"))
            .unwrap();
        series
            .insert_message(observed_domain_event(
                "event-1",
                "correlation-1",
                Some("command-1"),
            ))
            .unwrap();

        assert_eq!(
            series
                .messages()
                .get("command-1")
                .unwrap()
                .observation_order(),
            0
        );
        assert_eq!(series.command_outcomes()[0].observation_order(), 1);
        assert_eq!(
            series
                .messages()
                .get("event-1")
                .unwrap()
                .observation_order(),
            2
        );
        let value = serde_json::to_value(&series).unwrap();
        assert_eq!(value["messages"][1]["observationOrder"], 2);
        assert_eq!(
            serde_json::from_value::<ObservedMessageSeries>(value).unwrap(),
            series
        );
    }

    #[test]
    fn observed_domain_aggregate_is_complete_validated_and_part_of_duplicate_identity() {
        let aggregate = aggregate();
        let event = ObservedMessageNode::domain_event(
            "event-1",
            "correlation-1",
            Some("command-1".to_owned()),
            "bicycle-rented",
            1,
            Some(aggregate.clone()),
            Some(json!({ "bicycle_id": "bike-42" })),
        );
        let mut series = ObservedMessageSeries::new();
        assert_eq!(
            series.insert_message(event.clone()).unwrap(),
            MessageSeriesInsertOutcome::Inserted
        );
        assert_eq!(
            series.insert_message(event).unwrap(),
            MessageSeriesInsertOutcome::Duplicate
        );
        assert_eq!(
            series.messages().get("event-1").unwrap().aggregate(),
            Some(&aggregate)
        );
        assert_eq!(
            serde_json::to_value(&series).unwrap()["messages"][0]["aggregate"],
            json!({ "type": "bike-rental/rental-fleet", "id": "city-fleet" })
        );

        let conflicting = ObservedMessageNode::domain_event(
            "event-1",
            "correlation-1",
            Some("command-1".to_owned()),
            "bicycle-rented",
            1,
            Some(TestAggregate {
                aggregate_type: aggregate.aggregate_type,
                id: "other-fleet".to_owned(),
            }),
            Some(json!({ "bicycle_id": "bike-42" })),
        );
        assert!(matches!(
            series.insert_message(conflicting),
            Err(ObservedMessageSeriesError::MessageSeries(_))
        ));

        let invalid = ObservedMessageNode::domain_event(
            "invalid-event",
            "correlation-1",
            None,
            "bicycle-rented",
            1,
            Some(TestAggregate {
                aggregate_type: String::new(),
                id: "city-fleet".to_owned(),
            }),
            None,
        );
        assert_eq!(
            ObservedMessageSeries::try_from_parts([invalid], []),
            Err(ObservedMessageSeriesError::InvalidMessage {
                field: "aggregate.type"
            })
        );
        let invalid = ObservedMessageNode::domain_event(
            "invalid-event",
            "correlation-1",
            None,
            "bicycle-rented",
            1,
            Some(TestAggregate {
                aggregate_type: "bike-rental/rental-fleet".to_owned(),
                id: String::new(),
            }),
            None,
        );
        assert_eq!(
            ObservedMessageSeries::try_from_parts([invalid], []),
            Err(ObservedMessageSeriesError::InvalidMessage {
                field: "aggregate.id"
            })
        );
    }

    #[test]
    fn observation_deserialization_rejects_noncontiguous_or_reordered_ordinals() {
        let mut series = ObservedMessageSeries::new();
        series
            .insert_message(observed_command("command-1", "correlation-1"))
            .unwrap();
        series
            .insert_message(observed_domain_event(
                "event-1",
                "correlation-1",
                Some("command-1"),
            ))
            .unwrap();
        let mut value = serde_json::to_value(series).unwrap();
        value["messages"][1]["observationOrder"] = json!(3);
        assert!(serde_json::from_value::<ObservedMessageSeries>(value).is_err());
    }

    #[test]
    fn comparator_matches_by_causation_and_payload_subset() {
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_domain_event("event", Some("subject")),
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
            ],
            None,
            None,
        )
        .unwrap();
        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(observed_domain_event(
                "event-id",
                "correlation-1",
                Some("command-id"),
            ))
            .unwrap();
        observed
            .insert_message(observed_command("command-id", "correlation-1"))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("command-id", "correlation-1"))
            .unwrap();

        let comparison =
            compare_message_series(&graph, &observed, MessageSeriesComparisonContext::default());
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Passed);
        assert!(comparison.diagnostics.is_empty());
        assert_eq!(
            comparison
                .matches
                .iter()
                .map(|matched| (
                    matched.expected_key.as_str(),
                    matched.observed_message_id.as_str()
                ))
                .collect::<Vec<_>>(),
            [("event", "event-id"), ("subject", "command-id")]
        );
    }

    #[test]
    fn comparator_selects_duplicate_siblings_by_content_then_id_not_arrival() {
        let first = expected_domain_event("a-child", Some("subject"));
        let mut second = expected_domain_event("b-child", Some("subject"));
        if let ExpectedMessageNode::DomainEvent { payload, .. } = &mut second {
            *payload = Some(json!({ "bicycle_id": "bike-42" }));
        }
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
                first,
                second,
            ],
            None,
            None,
        )
        .unwrap();
        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(observed_command("root", "correlation-1"))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("root", "correlation-1"))
            .unwrap();
        observed
            .insert_message(observed_domain_event(
                "z-child",
                "correlation-1",
                Some("root"),
            ))
            .unwrap();
        observed
            .insert_message(observed_domain_event(
                "a-child-id",
                "correlation-1",
                Some("root"),
            ))
            .unwrap();

        let comparison =
            compare_message_series(&graph, &observed, MessageSeriesComparisonContext::default());
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Passed);
        assert!(comparison.matches.iter().any(|matched| {
            matched.expected_key == "a-child" && matched.observed_message_id == "a-child-id"
        }));
        assert!(comparison.matches.iter().any(|matched| {
            matched.expected_key == "b-child" && matched.observed_message_id == "z-child"
        }));
    }

    #[test]
    fn comparator_fails_closed_when_ambiguous_assignment_exceeds_its_work_limit() {
        const BRANCHES: usize = MAXIMUM_EXPECTED_SIBLINGS - 1;
        let mut expected = vec![expected_command(
            "subject",
            None,
            Some(json!({ "bicycle_id": "bike-42" })),
            TestOutcome::Accepted,
        )];
        for parent in 0..BRANCHES {
            let parent_key = format!("expected-parent-{parent}");
            expected.push(expected_domain_event(&parent_key, Some("subject")));
            for child in 0..BRANCHES {
                expected.push(expected_domain_event(
                    &format!("expected-child-{parent}-{child}"),
                    Some(&parent_key),
                ));
            }
        }
        let graph = MessageGraphDefinition::try_from_nodes(expected, None, None).unwrap();

        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(observed_command("root", "correlation-1"))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("root", "correlation-1"))
            .unwrap();
        for parent in 0..BRANCHES {
            let parent_id = format!("observed-parent-{parent}");
            observed
                .insert_message(observed_domain_event(
                    &parent_id,
                    "correlation-1",
                    Some("root"),
                ))
                .unwrap();
            for child in 0..BRANCHES {
                observed
                    .insert_message(observed_domain_event(
                        &format!("observed-child-{parent}-{child}"),
                        "correlation-1",
                        Some(&parent_id),
                    ))
                    .unwrap();
            }
        }

        let comparison =
            compare_message_series(&graph, &observed, MessageSeriesComparisonContext::default());
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Failed);
        assert!(
            comparison
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "comparison-work-limit-exceeded")
        );
    }

    #[test]
    fn comparator_finds_complete_broad_and_restrictive_sibling_assignments() {
        for (broad_key, restrictive_key, reverse_order) in [
            ("a-broad", "z-restrictive", false),
            ("z-broad", "a-restrictive", true),
        ] {
            let broad = ExpectedMessageNode::DomainEvent {
                key: broad_key.to_owned(),
                parent_key: Some("subject".to_owned()),
                name: "bicycle-rented".to_owned(),
                schema_version: 1,
                payload: Some(json!({ "bicycle_id": "bike-42" })),
            };
            let restrictive = ExpectedMessageNode::DomainEvent {
                key: restrictive_key.to_owned(),
                parent_key: Some("subject".to_owned()),
                name: "bicycle-rented".to_owned(),
                schema_version: 1,
                payload: Some(json!({ "bicycle_id": "bike-42", "slot": 7 })),
            };
            let mut nodes = vec![
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
                broad,
                restrictive,
            ];
            if reverse_order {
                nodes.swap(1, 2);
            }
            let graph = MessageGraphDefinition::try_from_nodes(nodes, None, None).unwrap();
            let mut observed = ObservedMessageSeries::new();
            observed
                .insert_message(observed_command("root", "correlation-1"))
                .unwrap();
            observed
                .insert_command_outcome(accepted_outcome("root", "correlation-1"))
                .unwrap();
            observed
                .insert_message(ObservedMessageNode::domain_event(
                    "a-restrictive-observation",
                    "correlation-1",
                    Some("root".to_owned()),
                    "bicycle-rented",
                    1,
                    None,
                    Some(json!({ "bicycle_id": "bike-42", "slot": 7 })),
                ))
                .unwrap();
            observed
                .insert_message(ObservedMessageNode::domain_event(
                    "z-broad-observation",
                    "correlation-1",
                    Some("root".to_owned()),
                    "bicycle-rented",
                    1,
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                ))
                .unwrap();

            let comparison = compare_message_series(
                &graph,
                &observed,
                MessageSeriesComparisonContext::default(),
            );
            assert_eq!(
                comparison.status,
                MessageSeriesComparisonStatus::Passed,
                "{comparison:#?}"
            );
            assert!(comparison.matches.iter().any(|matched| {
                matched.expected_key == restrictive_key
                    && matched.observed_message_id == "a-restrictive-observation"
            }));
            assert!(comparison.matches.iter().any(|matched| {
                matched.expected_key == broad_key
                    && matched.observed_message_id == "z-broad-observation"
            }));
        }
    }

    #[test]
    fn comparator_uses_descendants_to_assign_identical_siblings() {
        let parent = |key: &str| ExpectedMessageNode::DomainEvent {
            key: key.to_owned(),
            parent_key: Some("subject".to_owned()),
            name: "rental-progressed".to_owned(),
            schema_version: 1,
            payload: Some(json!({ "bicycle_id": "bike-42" })),
        };
        let child =
            |key: &str, parent_key: &str, name: &str| ExpectedMessageNode::IntegrationEvent {
                key: key.to_owned(),
                parent_key: Some(parent_key.to_owned()),
                name: name.to_owned(),
                schema_version: 1,
                payload: None,
            };
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
                parent("left-parent"),
                parent("right-parent"),
                child("left-child", "left-parent", "left-finished"),
                child("right-child", "right-parent", "right-finished"),
            ],
            None,
            None,
        )
        .unwrap();
        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(observed_command("root", "correlation-1"))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("root", "correlation-1"))
            .unwrap();
        for parent_id in ["a-parent", "z-parent"] {
            observed
                .insert_message(ObservedMessageNode::domain_event(
                    parent_id,
                    "correlation-1",
                    Some("root".to_owned()),
                    "rental-progressed",
                    1,
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                ))
                .unwrap();
        }
        observed
            .insert_message(ObservedMessageNode::integration_event(
                "a-child",
                "correlation-1",
                Some("a-parent".to_owned()),
                "right-finished",
                1,
                None,
            ))
            .unwrap();
        observed
            .insert_message(ObservedMessageNode::integration_event(
                "z-child",
                "correlation-1",
                Some("z-parent".to_owned()),
                "left-finished",
                1,
                None,
            ))
            .unwrap();

        let comparison =
            compare_message_series(&graph, &observed, MessageSeriesComparisonContext::default());
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Passed);
        assert!(comparison.matches.iter().any(|matched| {
            matched.expected_key == "left-parent" && matched.observed_message_id == "z-parent"
        }));
        assert!(comparison.matches.iter().any(|matched| {
            matched.expected_key == "right-parent" && matched.observed_message_id == "a-parent"
        }));
    }

    #[test]
    fn comparator_reports_stable_field_outcome_and_unexpected_diagnostics() {
        let graph = MessageGraphDefinition::try_from_nodes(
            [expected_command(
                "subject",
                None,
                Some(json!({ "bicycle_id": "bike-42" })),
                TestOutcome::Accepted,
            )],
            None,
            None,
        )
        .unwrap();
        let mut wrong = ObservedMessageNode::integration_event(
            "wrong",
            "correlation-1",
            None,
            "other-command",
            2,
            Some(json!({ "bicycle_id": "different" })),
        );
        wrong.set_observation_order(99);
        let mut observed = ObservedMessageSeries::new();
        observed.insert_message(wrong).unwrap();
        observed
            .insert_message(ObservedMessageNode::integration_event(
                "extra",
                "correlation-1",
                Some("wrong".to_owned()),
                "extra-event",
                1,
                None,
            ))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("extra", "correlation-1"))
            .unwrap();

        let comparison =
            compare_message_series(&graph, &observed, MessageSeriesComparisonContext::default());
        let codes = diagnostic_codes(&comparison);
        assert_eq!(comparison.status, MessageSeriesComparisonStatus::Failed);
        for code in [
            "aggregate-mismatch",
            "command-outcome-mismatch",
            "identity-conflict",
            "kind-mismatch",
            "name-mismatch",
            "payload-mismatch",
            "schema-version-mismatch",
            "unexpected-observed",
        ] {
            assert!(codes.contains(&code), "missing diagnostic {code}");
        }
    }

    #[test]
    fn comparator_reports_causation_topology_cycles_and_cross_correlation() {
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
                expected_domain_event("event", Some("subject")),
            ],
            None,
            None,
        )
        .unwrap();
        let mut unresolved = ObservedMessageSeries::new();
        unresolved
            .insert_message(observed_command("root", "correlation-1"))
            .unwrap();
        unresolved
            .insert_command_outcome(accepted_outcome("root", "correlation-1"))
            .unwrap();
        unresolved
            .insert_message(observed_domain_event(
                "event",
                "correlation-1",
                Some("wrong-parent"),
            ))
            .unwrap();
        let comparison = compare_message_series(
            &graph,
            &unresolved,
            MessageSeriesComparisonContext::default(),
        );
        let codes = diagnostic_codes(&comparison);
        assert!(codes.contains(&"causation-mismatch"));
        assert!(codes.contains(&"unresolved-observed-parent"));

        let cycle = ObservedMessageSeries::try_from_parts(
            [
                ObservedMessageNode::domain_event(
                    "a",
                    "correlation-1",
                    Some("b".to_owned()),
                    "a",
                    1,
                    None,
                    None,
                ),
                ObservedMessageNode::domain_event(
                    "b",
                    "correlation-2",
                    Some("a".to_owned()),
                    "b",
                    1,
                    None,
                    None,
                ),
            ],
            [],
        )
        .unwrap();
        let comparison =
            compare_message_series(&graph, &cycle, MessageSeriesComparisonContext::default());
        let codes = diagnostic_codes(&comparison);
        assert!(codes.contains(&"causation-cycle"));
        assert!(codes.contains(&"cross-correlation-link"));
    }

    #[test]
    fn comparator_reports_timeout_and_settling_boundaries_without_using_them_for_matching() {
        let graph = MessageGraphDefinition::try_from_nodes(
            [
                expected_command(
                    "subject",
                    None,
                    Some(json!({ "bicycle_id": "bike-42" })),
                    TestOutcome::Accepted,
                ),
                expected_domain_event("event", Some("subject")),
            ],
            None,
            None,
        )
        .unwrap();
        let mut observed = ObservedMessageSeries::new();
        observed
            .insert_message(observed_command("root", "correlation-1"))
            .unwrap();
        for command_id in ["during-command", "after-command", "timeout-command"] {
            let mut command = observed_command(command_id, "correlation-1");
            if let ObservedMessageNode::Command { causation_id, .. } = &mut command {
                *causation_id = Some("root".to_owned());
            }
            observed.insert_message(command).unwrap();
        }
        observed
            .insert_command_outcome(accepted_outcome("root", "correlation-1"))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("during-command", "correlation-1"))
            .unwrap();
        observed
            .insert_message(ObservedMessageNode::integration_event(
                "during",
                "correlation-1",
                Some("root".to_owned()),
                "during",
                1,
                None,
            ))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("after-command", "correlation-1"))
            .unwrap();
        observed
            .insert_message(ObservedMessageNode::integration_event(
                "after",
                "correlation-1",
                Some("root".to_owned()),
                "after",
                1,
                None,
            ))
            .unwrap();
        observed
            .insert_command_outcome(accepted_outcome("timeout-command", "correlation-1"))
            .unwrap();

        let comparison = compare_message_series(
            &graph,
            &observed,
            MessageSeriesComparisonContext {
                timed_out: true,
                timeout_observation_order: Some(8),
                settle_started_at_order: Some(4),
                settle_completed_at_order: Some(7),
            },
        );
        let codes = diagnostic_codes(&comparison);
        assert!(codes.contains(&"timeout-before-expectations"));
        assert!(codes.contains(&"message-during-settle"));
        assert!(codes.contains(&"message-after-settle"));
        assert!(codes.contains(&"message-after-timeout"));
        assert!(codes.contains(&"command-outcome-during-settle"));
        assert!(codes.contains(&"command-outcome-after-settle"));
        assert!(codes.contains(&"command-outcome-after-timeout"));
    }

    #[test]
    fn comparator_payload_semantics_are_recursive_subsets_with_exact_arrays_and_scalars() {
        let actual = json!({
            "nested": { "id": 1, "extra": true },
            "array": [{ "id": 1, "extra": true }],
            "scalar": "same"
        });
        assert!(crate::payload_matches_subset(
            &json!({ "nested": { "id": 1 } }),
            &actual
        ));
        assert!(crate::payload_matches_subset(
            &json!({ "array": [{ "id": 1 }] }),
            &actual
        ));
        assert!(!crate::payload_matches_subset(
            &json!({ "array": [{ "id": 1 }, { "id": 2 }] }),
            &actual
        ));
        assert!(!crate::payload_matches_subset(
            &json!({ "scalar": "different" }),
            &actual
        ));
    }
}
