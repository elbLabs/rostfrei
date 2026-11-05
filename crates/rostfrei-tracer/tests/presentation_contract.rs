//! Portable observed-v2 conformance cases, using the existing Rust wire reader as oracle.
//! `causationMatches` are identity equalities, including anomalous edges; diagnostics
//! must accompany them. This suite makes no original-byte/numeric-lexeme guarantee.
#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

use std::collections::{BTreeMap, HashSet};

use rostfrei_messaging_core::MessageSeriesTopologyIssue;
use rostfrei_tracer::{ObservedMessageSeries, ObservedMessageSeriesOutcomeIssue};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Corpus {
    format_version: u32,
    contract: String,
    bases: BTreeMap<String, Value>,
    cases: Vec<Case>,
    limits: Vec<Limit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    base: String,
    #[serde(default)]
    set: BTreeMap<String, Value>,
    #[serde(default)]
    remove: Vec<String>,
    expect: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limit {
    name: String,
    base: String,
    #[serde(flatten)]
    expansion: Expansion,
    structurally_valid: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Expansion {
    Array {
        field: String,
        count: usize,
    },
    String {
        path: String,
        unit: String,
        count: usize,
    },
}

fn corpus() -> Corpus {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/presentation-contract-v2.json")).unwrap();
    assert_eq!(corpus.format_version, 1);
    assert_eq!(corpus.contract, "observed-message-series-v2");
    let mut names = HashSet::new();
    for name in corpus
        .cases
        .iter()
        .map(|case| &case.name)
        .chain(corpus.limits.iter().map(|case| &case.name))
    {
        assert!(names.insert(name), "duplicate fixture case: {name}");
    }
    corpus
}

// Set/remove only object members; parent paths must exist. Paths are JSON Pointers.
fn member<'a>(
    document: &'a mut Value,
    path: &str,
) -> (&'a mut serde_json::Map<String, Value>, String) {
    let (parent, key) = path.rsplit_once('/').unwrap();
    let object = document
        .pointer_mut(parent)
        .unwrap()
        .as_object_mut()
        .unwrap();
    (object, key.replace("~1", "/").replace("~0", "~"))
}

fn set(document: &mut Value, path: &str, value: Value) {
    let (object, key) = member(document, path);
    object.insert(key, value);
}

fn facts(series: &ObservedMessageSeries) -> Value {
    let messages = series.messages();
    let causation_matches: Vec<_> = messages
        .iter()
        .filter_map(|child| {
            let parent = messages.get(child.causation_id()?)?;
            Some([parent.message_id(), child.message_id()])
        })
        .collect();
    let outcome_associations: Vec<_> = series
        .command_outcomes()
        .iter()
        .filter_map(|outcome| {
            let command = messages.get(outcome.command_message_id())?;
            (command.is_command() && command.correlation_id() == outcome.correlation_id())
                .then_some([command.message_id(), outcome.response_message_id()])
        })
        .collect();
    let topology_issues: Vec<_> = messages
        .topology_issues()
        .into_iter()
        .map(|issue| match issue {
            MessageSeriesTopologyIssue::UnresolvedParent { child } => json!({
                "kind": "missingParent", "child": child.message_id(), "parent": child.causation_id(),
            }),
            MessageSeriesTopologyIssue::CrossCorrelation { child, parent } => json!({
                "kind": "crossCorrelation", "child": child.message_id(), "parent": parent.message_id(),
            }),
            MessageSeriesTopologyIssue::Cycle { nodes } => json!({
                "kind": "cycle", "messages": nodes.iter().map(|node| node.message_id()).collect::<Vec<_>>(),
            }),
            _ => json!({"kind": "unhandledTopologyIssue"}),
        })
        .collect();
    let outcome_issues: Vec<_> = series
        .outcome_issues()
        .into_iter()
        .map(|issue| {
            let (kind, outcome) = match issue {
                ObservedMessageSeriesOutcomeIssue::MissingCommand { outcome } => {
                    ("missingCommand", outcome)
                }
                ObservedMessageSeriesOutcomeIssue::NotACommand { outcome, .. } => {
                    ("notACommand", outcome)
                }
                ObservedMessageSeriesOutcomeIssue::CrossCorrelation { outcome, .. } => {
                    ("crossCorrelation", outcome)
                }
                _ => return json!({"kind": "unhandledOutcomeIssue"}),
            };
            json!({"kind": kind, "command": outcome.command_message_id(), "response": outcome.response_message_id()})
        })
        .collect();
    json!({
        "structurallyValid": true,
        "causationMatches": causation_matches,
        "outcomeAssociations": outcome_associations,
        "topologyIssues": topology_issues,
        "outcomeIssues": outcome_issues,
    })
}

#[test]
fn canonical_wire_cases_and_diagnostics() {
    let corpus = corpus();
    assert!(!corpus.cases.is_empty());
    for case in &corpus.cases {
        let mut document = corpus.bases[&case.base].clone();
        for (path, value) in &case.set {
            set(&mut document, path, value.clone());
        }
        for path in &case.remove {
            let (object, key) = member(&mut document, path);
            assert!(
                object.remove(&key).is_some(),
                "{}: missing {path}",
                case.name
            );
        }
        // Deserialize serialized ordinals; insertion constructors would silently reassign them.
        let result = serde_json::from_value::<ObservedMessageSeries>(document);
        let actual = result
            .as_ref()
            .map_or_else(|_| json!({"structurallyValid": false}), facts);
        assert_eq!(actual, case.expect, "{}: {result:?}", case.name);
    }
    println!("{} canonical wire/diagnostic cases", corpus.cases.len());
}

#[test]
fn canonical_array_and_utf8_bounds() {
    let corpus = corpus();
    assert!(!corpus.limits.is_empty());
    for case in &corpus.limits {
        let mut document = corpus.bases[&case.base].clone();
        match &case.expansion {
            Expansion::Array { field, count } => {
                assert!(matches!(field.as_str(), "messages" | "commandOutcomes"));
                let template = document[field][0].clone();
                document["messages"] = json!([]);
                document["commandOutcomes"] = json!([]);
                document[field] = Value::Array(
                    (0..*count)
                        .map(|index| {
                            let mut record = template.clone();
                            record["observationOrder"] = json!(index);
                            if field == "messages" {
                                record["messageId"] = json!(format!("m-{index}"));
                            } else {
                                record["commandMessageId"] = json!(format!("c-{index}"));
                                record["responseMessageId"] = json!(format!("r-{index}"));
                            }
                            record
                        })
                        .collect(),
                );
            }
            Expansion::String { path, unit, count } => {
                set(&mut document, path, json!(unit.repeat(*count)));
            }
        }
        let result = serde_json::from_value::<ObservedMessageSeries>(document);
        assert_eq!(
            result.is_ok(),
            case.structurally_valid,
            "{}: {}",
            case.name,
            result
                .err()
                .map_or_else(String::new, |error| error.to_string()),
        );
    }
    println!(
        "{} canonical array/string boundary cases",
        corpus.limits.len()
    );
}
