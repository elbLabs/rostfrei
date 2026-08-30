use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::Hash,
    marker::PhantomData,
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, SeqAccess, Visitor},
};

use crate::{ContractError, ContractErrorKind};

pub const MAX_MESSAGE_SERIES_NODES: usize = 4096;

/// A message that can participate in a causal series.
///
/// Identity and relationship values must remain stable while the node is in a
/// [`MessageSeries`]. The series intentionally exposes no mutable node access.
pub trait MessageSeriesNode: Eq {
    type MessageId: Eq + Hash + ?Sized;
    type CorrelationId: Eq + ?Sized;

    fn message_id(&self) -> &Self::MessageId;
    fn correlation_id(&self) -> &Self::CorrelationId;
    fn causation_id(&self) -> Option<&Self::MessageId>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageSeriesInsertOutcome {
    Inserted,
    Duplicate,
}

impl MessageSeriesInsertOutcome {
    pub const fn is_duplicate(self) -> bool {
        matches!(self, Self::Duplicate)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum MessageSeriesTopologyIssue<'a, N> {
    UnresolvedParent { child: &'a N },
    CrossCorrelation { child: &'a N, parent: &'a N },
    Cycle { nodes: Vec<&'a N> },
}

/// An insertion-ordered, partial-capable causal graph of messages.
///
/// Array position records observation order only. Causality is defined solely
/// by [`MessageSeriesNode::causation_id`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MessageSeries<N> {
    nodes: Vec<N>,
}

impl<N> MessageSeries<N> {
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &N> {
        self.nodes.iter()
    }

    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn into_nodes(self) -> Vec<N> {
        self.nodes
    }
}

impl<N> Default for MessageSeries<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N> MessageSeries<N>
where
    N: MessageSeriesNode,
{
    pub fn try_from_nodes<I>(nodes: I) -> Result<Self, ContractError>
    where
        I: IntoIterator<Item = N>,
    {
        let mut series = Self::new();
        for node in nodes {
            series.insert(node)?;
        }
        Ok(series)
    }

    pub fn insert(&mut self, node: N) -> Result<MessageSeriesInsertOutcome, ContractError> {
        if let Some(existing) = self.get(node.message_id()) {
            return if existing == &node {
                Ok(MessageSeriesInsertOutcome::Duplicate)
            } else {
                Err(ContractError::new(
                    ContractErrorKind::IdentityConflict,
                    "message series node",
                ))
            };
        }
        if self.nodes.len() == MAX_MESSAGE_SERIES_NODES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooManyEntries,
                "message series",
                MAX_MESSAGE_SERIES_NODES.saturating_add(1),
                MAX_MESSAGE_SERIES_NODES,
            ));
        }

        self.nodes.push(node);
        Ok(MessageSeriesInsertOutcome::Inserted)
    }

    pub fn get(&self, message_id: &N::MessageId) -> Option<&N> {
        self.nodes
            .iter()
            .find(|node| node.message_id() == message_id)
    }

    pub fn roots(&self) -> impl Iterator<Item = &N> {
        self.nodes
            .iter()
            .filter(|node| node.causation_id().is_none())
    }

    pub fn direct_children<'a>(
        &'a self,
        message_id: &'a N::MessageId,
    ) -> impl Iterator<Item = &'a N> + 'a {
        self.nodes
            .iter()
            .filter(move |node| node.causation_id() == Some(message_id))
    }

    pub fn unresolved_nodes(&self) -> impl Iterator<Item = &N> {
        let identities = self
            .nodes
            .iter()
            .map(MessageSeriesNode::message_id)
            .collect::<HashSet<_>>();
        self.nodes.iter().filter(move |node| {
            node.causation_id()
                .is_some_and(|causation_id| !identities.contains(causation_id))
        })
    }

    /// Reports unresolved and cross-correlation edges in child observation
    /// order, followed by cycles ordered by their earliest observed member.
    pub fn topology_issues(&self) -> Vec<MessageSeriesTopologyIssue<'_, N>> {
        let indexes = node_indexes(&self.nodes);
        let mut issues = Vec::new();
        let parent_indexes = self
            .nodes
            .iter()
            .map(|child| {
                let causation_id = child.causation_id()?;
                let Some(parent_index) = indexes.get(causation_id).copied() else {
                    issues.push(MessageSeriesTopologyIssue::UnresolvedParent { child });
                    return None;
                };
                let parent = self.nodes.get(parent_index)?;
                if child.correlation_id() != parent.correlation_id() {
                    issues.push(MessageSeriesTopologyIssue::CrossCorrelation { child, parent });
                }
                Some(parent_index)
            })
            .collect::<Vec<_>>();

        for cycle in find_cycles(&parent_indexes) {
            issues.push(MessageSeriesTopologyIssue::Cycle {
                nodes: cycle
                    .into_iter()
                    .filter_map(|index| self.nodes.get(index))
                    .collect(),
            });
        }
        issues
    }
}

impl<'de, N> Deserialize<'de> for MessageSeries<N>
where
    N: Deserialize<'de> + MessageSeriesNode,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(MessageSeriesVisitor(PhantomData))
    }
}

struct MessageSeriesVisitor<N>(PhantomData<fn() -> N>);

impl<'de, N> Visitor<'de> for MessageSeriesVisitor<N>
where
    N: Deserialize<'de> + MessageSeriesNode,
{
    type Value = MessageSeries<N>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of message series nodes")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let capacity = sequence
            .size_hint()
            .unwrap_or(0)
            .min(MAX_MESSAGE_SERIES_NODES);
        let mut series = MessageSeries {
            nodes: Vec::with_capacity(capacity),
        };
        while let Some(node) = sequence.next_element()? {
            series.insert(node).map_err(A::Error::custom)?;
        }
        Ok(series)
    }
}

fn node_indexes<N>(nodes: &[N]) -> HashMap<&N::MessageId, usize>
where
    N: MessageSeriesNode,
{
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.message_id(), index))
        .collect()
}

fn find_cycles(parent_indexes: &[Option<usize>]) -> Vec<Vec<usize>> {
    const UNVISITED: u8 = 0;
    const VISITING: u8 = 1;
    const VISITED: u8 = 2;

    let mut states = vec![UNVISITED; parent_indexes.len()];
    let mut cycles = Vec::new();
    for start in 0..parent_indexes.len() {
        if states.get(start).copied() != Some(UNVISITED) {
            continue;
        }

        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            let Some(state) = states.get(index).copied() else {
                break;
            };
            if state != UNVISITED {
                if state == VISITING
                    && let Some(cycle_start) = path.iter().position(|candidate| *candidate == index)
                {
                    let mut cycle = path.get(cycle_start..).unwrap_or_default().to_vec();
                    if let Some((earliest, _)) = cycle
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, node_index)| **node_index)
                    {
                        cycle.rotate_left(earliest);
                    }
                    cycles.push(cycle);
                }
                break;
            }

            let Some(state) = states.get_mut(index) else {
                break;
            };
            *state = VISITING;
            path.push(index);
            current = parent_indexes.get(index).copied().flatten();
        }
        for index in path {
            if let Some(state) = states.get_mut(index) {
                *state = VISITED;
            }
        }
    }
    cycles.sort_by_key(|cycle| cycle.first().copied().unwrap_or(usize::MAX));
    cycles
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct TestNode {
        message_id: String,
        correlation_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        causation_id: Option<String>,
        payload: String,
    }

    impl MessageSeriesNode for TestNode {
        type CorrelationId = str;
        type MessageId = str;

        fn message_id(&self) -> &Self::MessageId {
            &self.message_id
        }

        fn correlation_id(&self) -> &Self::CorrelationId {
            &self.correlation_id
        }

        fn causation_id(&self) -> Option<&Self::MessageId> {
            self.causation_id.as_deref()
        }
    }

    fn node(message_id: &str, correlation_id: &str, causation_id: Option<&str>) -> TestNode {
        TestNode {
            message_id: message_id.to_owned(),
            correlation_id: correlation_id.to_owned(),
            causation_id: causation_id.map(str::to_owned),
            payload: format!("payload-{message_id}"),
        }
    }

    fn message_ids<'a>(nodes: impl Iterator<Item = &'a TestNode>) -> Vec<&'a str> {
        nodes.map(|node| node.message_id.as_str()).collect()
    }

    #[test]
    fn preserves_observation_order_and_derives_branching_graphs() {
        let series = MessageSeries::try_from_nodes([
            node("root-1", "correlation-1", None),
            node("child-2", "correlation-1", Some("root-1")),
            node("child-1", "correlation-1", Some("root-1")),
            node("root-2", "correlation-2", None),
        ])
        .unwrap();

        assert_eq!(
            message_ids(series.iter()),
            ["root-1", "child-2", "child-1", "root-2"]
        );
        assert_eq!(message_ids(series.roots()), ["root-1", "root-2"]);
        assert_eq!(
            message_ids(series.direct_children("root-1")),
            ["child-2", "child-1"]
        );
        assert_eq!(series.get("child-1").unwrap().payload, "payload-child-1");
        assert!(series.unresolved_nodes().next().is_none());
        assert!(series.topology_issues().is_empty());
    }

    #[test]
    fn retains_an_unresolved_child_and_resolves_it_when_the_parent_arrives() {
        let child = node("child", "correlation-1", Some("parent"));
        let mut series = MessageSeries::try_from_nodes([child.clone()]).unwrap();

        assert_eq!(message_ids(series.unresolved_nodes()), ["child"]);
        assert_eq!(
            series.topology_issues(),
            [MessageSeriesTopologyIssue::UnresolvedParent { child: &child }]
        );
        assert_eq!(message_ids(series.direct_children("parent")), ["child"]);

        series
            .insert(node("parent", "correlation-1", None))
            .unwrap();

        assert!(series.unresolved_nodes().next().is_none());
        assert!(series.topology_issues().is_empty());
        assert_eq!(message_ids(series.iter()), ["child", "parent"]);
    }

    #[test]
    fn duplicate_insertion_is_idempotent_but_conflicting_content_is_rejected() {
        let original = node("message-1", "correlation-1", None);
        let mut series = MessageSeries::try_from_nodes([original.clone()]).unwrap();

        assert_eq!(
            series.insert(original.clone()).unwrap(),
            MessageSeriesInsertOutcome::Duplicate
        );
        assert_eq!(series.len(), 1);

        let mut conflicting = original;
        conflicting.payload = "changed".to_owned();
        let error = series.insert(conflicting).unwrap_err();

        assert_eq!(error.kind(), ContractErrorKind::IdentityConflict);
        assert_eq!(error.field(), "message series node");
        assert_eq!(series.len(), 1);
    }

    #[test]
    fn enforces_the_node_limit_after_duplicate_detection() {
        let nodes = (0..MAX_MESSAGE_SERIES_NODES)
            .map(|index| node(&format!("message-{index}"), "correlation-1", None));
        let mut series = MessageSeries::try_from_nodes(nodes).unwrap();

        assert_eq!(
            series
                .insert(node("message-0", "correlation-1", None))
                .unwrap(),
            MessageSeriesInsertOutcome::Duplicate
        );
        let error = series
            .insert(node("one-too-many", "correlation-1", None))
            .unwrap_err();

        assert_eq!(error.kind(), ContractErrorKind::TooManyEntries);
        assert_eq!(error.actual(), Some(MAX_MESSAGE_SERIES_NODES + 1));
        assert_eq!(error.maximum(), Some(MAX_MESSAGE_SERIES_NODES));
        assert_eq!(series.len(), MAX_MESSAGE_SERIES_NODES);
    }

    #[test]
    fn reports_cross_correlation_edges_and_cycles_without_dropping_nodes() {
        let first = node("first", "correlation-1", Some("second"));
        let second = node("second", "correlation-2", Some("first"));
        let series = MessageSeries::try_from_nodes([first.clone(), second.clone()]).unwrap();

        assert_eq!(
            series.topology_issues(),
            [
                MessageSeriesTopologyIssue::CrossCorrelation {
                    child: &first,
                    parent: &second,
                },
                MessageSeriesTopologyIssue::CrossCorrelation {
                    child: &second,
                    parent: &first,
                },
                MessageSeriesTopologyIssue::Cycle {
                    nodes: vec![&first, &second],
                },
            ]
        );
        assert_eq!(message_ids(series.iter()), ["first", "second"]);
    }

    #[test]
    fn reports_self_causation_as_a_single_node_cycle() {
        let self_caused = node("self", "correlation-1", Some("self"));
        let series = MessageSeries::try_from_nodes([self_caused.clone()]).unwrap();

        assert_eq!(
            series.topology_issues(),
            [MessageSeriesTopologyIssue::Cycle {
                nodes: vec![&self_caused],
            }]
        );
    }

    #[test]
    fn reports_only_cycle_members_for_tails_and_orders_disjoint_cycles() {
        let series = MessageSeries::try_from_nodes([
            node("tail", "correlation-1", Some("cycle-b")),
            node("cycle-a", "correlation-1", Some("cycle-b")),
            node("cycle-b", "correlation-1", Some("cycle-a")),
            node("second-a", "correlation-2", Some("second-b")),
            node("second-b", "correlation-2", Some("second-c")),
            node("second-c", "correlation-2", Some("second-a")),
            node("unresolved", "correlation-3", Some("missing")),
        ])
        .unwrap();

        let cycles = series
            .topology_issues()
            .into_iter()
            .filter_map(|issue| match issue {
                MessageSeriesTopologyIssue::Cycle { nodes } => Some(message_ids(nodes.into_iter())),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            cycles,
            [
                vec!["cycle-a", "cycle-b"],
                vec!["second-a", "second-b", "second-c"],
            ]
        );
    }

    #[test]
    fn serializes_as_an_array_and_revalidates_during_deserialization() {
        let root = node("root", "correlation-1", None);
        let child = node("child", "correlation-1", Some("root"));
        let series = MessageSeries::try_from_nodes([root.clone(), child.clone()]).unwrap();

        let value = serde_json::to_value(&series).unwrap();
        assert_eq!(value, serde_json::json!([root, child]));
        assert_eq!(
            serde_json::from_value::<MessageSeries<TestNode>>(value).unwrap(),
            series
        );

        let duplicated = serde_json::json!([
            node("message", "correlation-1", None),
            node("message", "correlation-1", None)
        ]);
        assert_eq!(
            serde_json::from_value::<MessageSeries<TestNode>>(duplicated)
                .unwrap()
                .len(),
            1
        );

        let mut conflict = node("message", "correlation-1", None);
        conflict.payload = "changed".to_owned();
        let conflicting = serde_json::json!([node("message", "correlation-1", None), conflict]);
        let error = serde_json::from_value::<MessageSeries<TestNode>>(conflicting).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("conflicts with an existing identity")
        );
    }

    #[test]
    fn deserialization_enforces_the_node_limit() {
        let maximum = (0..MAX_MESSAGE_SERIES_NODES)
            .map(|index| node(&format!("message-{index}"), "correlation-1", None))
            .collect::<Vec<_>>();
        let serialized = serde_json::to_value(&maximum).unwrap();
        assert_eq!(
            serde_json::from_value::<MessageSeries<TestNode>>(serialized)
                .unwrap()
                .len(),
            MAX_MESSAGE_SERIES_NODES
        );

        let mut oversized = maximum;
        oversized.push(node("one-too-many", "correlation-1", None));
        let serialized = serde_json::to_value(oversized).unwrap();
        let error = serde_json::from_value::<MessageSeries<TestNode>>(serialized).unwrap_err();
        assert!(error.to_string().contains("contains too many entries"));
    }

    #[test]
    fn supports_owned_identity_types_without_pointer_identity() {
        #[derive(Clone, Debug, Eq, Hash, PartialEq)]
        struct TestId(String);

        #[derive(Clone, Debug, Eq, PartialEq)]
        struct OwnedIdentityNode {
            message: TestId,
            correlation: TestId,
            causation: Option<TestId>,
        }

        impl MessageSeriesNode for OwnedIdentityNode {
            type CorrelationId = TestId;
            type MessageId = TestId;

            fn message_id(&self) -> &Self::MessageId {
                &self.message
            }

            fn correlation_id(&self) -> &Self::CorrelationId {
                &self.correlation
            }

            fn causation_id(&self) -> Option<&Self::MessageId> {
                self.causation.as_ref()
            }
        }

        let parent_id = TestId("parent".to_owned());
        let child = OwnedIdentityNode {
            message: TestId("child".to_owned()),
            correlation: TestId("correlation".to_owned()),
            causation: Some(TestId("parent".to_owned())),
        };
        let parent = OwnedIdentityNode {
            message: TestId("parent".to_owned()),
            correlation: TestId("correlation".to_owned()),
            causation: None,
        };
        let series = MessageSeries::try_from_nodes([child, parent]).unwrap();

        assert_eq!(series.get(&parent_id).unwrap().message, parent_id);
        assert_eq!(series.direct_children(&parent_id).count(), 1);
        assert!(series.unresolved_nodes().next().is_none());
        assert!(series.topology_issues().is_empty());
    }
}
