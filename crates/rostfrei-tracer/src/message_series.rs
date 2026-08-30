use std::{
    collections::{HashMap, hash_map::Entry},
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
    #[schemars(with = "Vec<ExpectedMessageNode>", length(min = 1, max = 4096))]
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
        payload: Option<Value>,
        outcome: ExpectedOutcomeWire,
    },
    DomainEvent {
        key: String,
        parent_key: Option<String>,
        name: String,
        schema_version: u32,
        payload: Option<Value>,
    },
    IntegrationEvent {
        key: String,
        parent_key: Option<String>,
        name: String,
        schema_version: u32,
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
    payload: Option<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
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
        #[schemars(range(min = 1))]
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
        #[schemars(range(min = 1))]
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
        #[schemars(range(min = 1))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
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

    fn name(&self) -> &str {
        match self {
            Self::Command { name, .. }
            | Self::DomainEvent { name, .. }
            | Self::IntegrationEvent { name, .. } => name,
        }
    }

    const fn schema_version(&self) -> u32 {
        match self {
            Self::Command { schema_version, .. }
            | Self::DomainEvent { schema_version, .. }
            | Self::IntegrationEvent { schema_version, .. } => *schema_version,
        }
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
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1))]
        schema_version: u32,
        #[schemars(with = "AggregateSchema")]
        aggregate: TestAggregate,
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
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
        #[serde(deserialize_with = "deserialize_nonempty_observed_name")]
        #[schemars(with = "NonEmptyStringSchema")]
        name: String,
        #[serde(deserialize_with = "deserialize_positive_observed_schema_version")]
        #[schemars(range(min = 1))]
        schema_version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
}

impl ObservedMessageNode {
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

    pub const fn outcome(&self) -> &CommandResponseOutcome {
        &self.outcome
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
}

impl ObservedMessageSeries {
    pub const fn new() -> Self {
        Self {
            messages: MessageSeries::new(),
            command_outcomes: Vec::new(),
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
        message: ObservedMessageNode,
    ) -> Result<MessageSeriesInsertOutcome, ObservedMessageSeriesError> {
        validate_observed_message(&message)?;
        self.messages
            .insert(message)
            .map_err(ObservedMessageSeriesError::MessageSeries)
    }

    pub fn insert_command_outcome(
        &mut self,
        outcome: ObservedCommandOutcome,
    ) -> Result<MessageSeriesInsertOutcome, ObservedMessageSeriesError> {
        validate_observed_outcome(&outcome)?;
        if let Some(existing) = self
            .command_outcomes
            .iter()
            .find(|existing| existing.command_message_id() == outcome.command_message_id())
        {
            return if existing == &outcome {
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
        Self::try_from_parts(wire.messages, wire.command_outcomes).map_err(D::Error::custom)
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
    TooManyMessages,
    TooManyOutcomes,
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
            Self::TooManyMessages => {
                formatter.write_str("observed message series contains more than 4096 messages")
            }
            Self::TooManyOutcomes => formatter
                .write_str("observed message series contains more than 4096 command outcomes"),
        }
    }
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
        }
        ObservedMessageNode::DomainEvent {
            name,
            schema_version,
            ..
        }
        | ObservedMessageNode::IntegrationEvent {
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

pub fn message_series_definition_schema() -> Schema {
    schema_for!(MessageSeriesDefinition)
}

pub fn observed_message_series_schema() -> Schema {
    schema_for!(ObservedMessageSeries)
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
    issues
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
    use rostfrei_messaging_core::MessageSeriesInsertOutcome;
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
        ObservedMessageNode::Command {
            message_id: message_id.to_owned(),
            correlation_id: correlation_id.to_owned(),
            causation_id: None,
            name: "rent-bicycle".to_owned(),
            schema_version: 1,
            aggregate: aggregate(),
            payload: Some(json!({ "bicycle_id": "bike-42" })),
        }
    }

    fn observed_domain_event(
        message_id: &str,
        correlation_id: &str,
        causation_id: Option<&str>,
    ) -> ObservedMessageNode {
        ObservedMessageNode::DomainEvent {
            message_id: message_id.to_owned(),
            correlation_id: correlation_id.to_owned(),
            causation_id: causation_id.map(str::to_owned),
            name: "bicycle-rented".to_owned(),
            schema_version: 1,
            payload: None,
        }
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
        series.insert_command_outcome(outcome.clone()).unwrap();

        assert_eq!(series.messages().unresolved_nodes().count(), 1);
        assert_eq!(
            series.outcome_issues(),
            [ObservedMessageSeriesOutcomeIssue::MissingCommand { outcome: &outcome }]
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
    fn observed_contract_rejects_invalid_values_and_raw_array_overflow() {
        let invalid_message = ObservedMessageNode::DomainEvent {
            message_id: String::new(),
            correlation_id: "correlation-1".to_owned(),
            causation_id: None,
            name: "bicycle-rented".to_owned(),
            schema_version: 1,
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
}
