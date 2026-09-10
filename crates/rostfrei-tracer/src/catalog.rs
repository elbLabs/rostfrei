use std::{collections::BTreeMap, fmt::Write as _};

use rostfrei_registry::DomainRegistry;
use serde::Serialize;
use serde_json::{Map, Value, json};

const CATALOG_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracerCatalog {
    pub catalog_version: u32,
    pub contexts: Vec<CatalogContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavioral_test: Option<CatalogBehavioralTest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_scenario: Option<CatalogTestScenario>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_repository: Option<CatalogTestRepository>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<CatalogQuarantine>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogQuarantine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<CatalogQuarantineScope>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogQuarantineScope {
    pub application: String,
    pub list_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBehavioralTest {
    pub schema_href: String,
    pub validate_href: String,
    pub run_href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definitions_href: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTestScenario {
    pub reset_href: String,
    pub fixture_reset_href_template: String,
    pub fixtures_href: String,
    pub fixtures: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFixtureCollection {
    pub items: Vec<TestFixtureSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFixtureSummary {
    pub id: String,
    pub revision: String,
    pub fixture_href: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTestRepository {
    pub definitions_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogContext {
    pub id: String,
    pub label: String,
    pub commands: Vec<CatalogCommand>,
    pub aggregates: Vec<CatalogAggregate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAggregate {
    pub id: String,
    pub label: String,
    pub aggregate_type: String,
    pub test_instances_href: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCommand {
    pub id: String,
    pub label: String,
    pub versions: Vec<CatalogCommandVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCommandVersion {
    pub schema_version: u32,
    pub content_type: &'static str,
    pub fields: Vec<Value>,
    pub payload_template: Value,
    pub test_inputs_href_template: String,
    pub simulate_href_template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_href_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_href_template: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateInstanceCollection {
    pub items: Vec<AggregateInstanceSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateInstanceSummary {
    pub aggregate_id: String,
    pub stream_version: u64,
}

#[derive(Default)]
struct ContextBuilder {
    label: String,
    commands: BTreeMap<String, CommandBuilder>,
    aggregates: BTreeMap<String, AggregateBuilder>,
}

struct AggregateBuilder {
    label: String,
    aggregate_type: String,
}

#[derive(Default)]
struct CommandBuilder {
    label: String,
    versions: Vec<CatalogCommandVersion>,
}

#[allow(clippy::fn_params_excessive_bools, clippy::too_many_lines)]
pub fn build_catalog<'a>(
    registry: &DomainRegistry,
    domain_model: Option<&Value>,
    test_enabled: bool,
    dispatch_enabled: bool,
    reset_enabled: bool,
    test_fixture_ids: impl IntoIterator<Item = &'a str>,
    test_repository_enabled: bool,
) -> TracerCatalog {
    let mut test_fixtures = test_fixture_ids
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    test_fixtures.sort();
    let mut contexts = BTreeMap::<String, ContextBuilder>::new();
    for descriptor in registry.commands() {
        let context_id = descriptor.bounded_context.to_owned();
        let context_label =
            model_context_label(domain_model, &context_id).unwrap_or_else(|| context_id.clone());
        let command_label = descriptor.modeled_command().label.to_owned();
        let fields = command_fields(descriptor.modeled_command().fields);
        let payload_template = payload_template(&fields);
        let context = contexts
            .entry(context_id.clone())
            .or_insert_with(|| ContextBuilder {
                label: context_label,
                commands: BTreeMap::new(),
                aggregates: BTreeMap::new(),
            });
        let command = context
            .commands
            .entry(descriptor.command_name.to_owned())
            .or_insert_with(|| CommandBuilder {
                label: command_label,
                versions: Vec::new(),
            });
        command.versions.push(CatalogCommandVersion {
            schema_version: descriptor.schema_version,
            content_type: "application/json",
            fields,
            payload_template,
            test_inputs_href_template: format!(
                "/contexts/{context_id}/commands/{}/schemas/{}/inputs",
                descriptor.command_name, descriptor.schema_version
            ),
            simulate_href_template: format!(
                "/contexts/{context_id}/commands/{}/simulate",
                descriptor.command_name
            ),
            test_href_template: test_enabled.then(|| {
                format!(
                    "/contexts/{context_id}/commands/{}/test",
                    descriptor.command_name
                )
            }),
            dispatch_href_template: dispatch_enabled.then(|| {
                format!(
                    "/contexts/{context_id}/commands/{}/dispatch",
                    descriptor.command_name
                )
            }),
        });
    }

    for aggregate_type in registry.aggregates() {
        let Some((context_id, aggregate_id)) = aggregate_type.split_once('/') else {
            continue;
        };
        if context_id.is_empty() || aggregate_id.is_empty() || aggregate_id.contains('/') {
            continue;
        }
        let context = contexts
            .entry(context_id.to_owned())
            .or_insert_with(|| ContextBuilder {
                label: model_context_label(domain_model, context_id)
                    .unwrap_or_else(|| context_id.to_owned()),
                commands: BTreeMap::new(),
                aggregates: BTreeMap::new(),
            });
        context
            .aggregates
            .entry(aggregate_id.to_owned())
            .or_insert_with(|| AggregateBuilder {
                label: model_aggregate_label(domain_model, context_id, aggregate_id)
                    .unwrap_or_else(|| aggregate_id.to_owned()),
                aggregate_type: aggregate_type.to_owned(),
            });
    }

    TracerCatalog {
        catalog_version: CATALOG_VERSION,
        quarantine: None,
        contexts: contexts
            .into_iter()
            .map(|(id, context)| CatalogContext {
                id,
                label: context.label,
                commands: context
                    .commands
                    .into_iter()
                    .map(|(id, mut command)| {
                        command
                            .versions
                            .sort_by_key(|version| version.schema_version);
                        CatalogCommand {
                            id,
                            label: command.label,
                            versions: command.versions,
                        }
                    })
                    .collect(),
                aggregates: context
                    .aggregates
                    .into_iter()
                    .map(|(id, aggregate)| {
                        let context_id = aggregate
                            .aggregate_type
                            .split_once('/')
                            .map_or("", |(context, _)| context);
                        CatalogAggregate {
                            test_instances_href: format!(
                                "/contexts/{context_id}/aggregates/{id}/instances"
                            ),
                            id,
                            label: aggregate.label,
                            aggregate_type: aggregate.aggregate_type,
                        }
                    })
                    .collect(),
            })
            .collect(),
        behavioral_test: (test_enabled && reset_enabled).then(|| CatalogBehavioralTest {
            schema_href: "/schemas/behavioral-test-v2".to_owned(),
            validate_href: "/tests/validate".to_owned(),
            run_href: "/test-runs".to_owned(),
            definitions_href: test_repository_enabled.then(|| "/tests".to_owned()),
        }),
        test_scenario: reset_enabled.then(|| CatalogTestScenario {
            reset_href: "/test-scenario/reset".to_owned(),
            fixture_reset_href_template: "/test-scenario/reset/{fixtureId}".to_owned(),
            fixtures_href: "/test-scenario/fixtures".to_owned(),
            fixtures: test_fixtures,
        }),
        test_repository: test_repository_enabled.then(|| CatalogTestRepository {
            definitions_href: "/tests".to_owned(),
        }),
    }
}

pub fn test_fixture_href(fixture_id: &str) -> String {
    format!(
        "/test-scenario/fixtures/{}",
        encode_path_segment(fixture_id)
    )
}

pub fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(&mut encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn model_context_label(model: Option<&Value>, context: &str) -> Option<String> {
    model?
        .get("boundedContexts")?
        .as_array()?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(context))?
        .get("label")?
        .as_str()
        .map(str::to_owned)
}

fn model_aggregate_label(model: Option<&Value>, context: &str, aggregate: &str) -> Option<String> {
    model?
        .get("aggregates")?
        .as_array()?
        .iter()
        .find(|item| {
            item.pointer("/id/context").and_then(Value::as_str) == Some(context)
                && item.pointer("/id/local").and_then(Value::as_str) == Some(aggregate)
        })?
        .get("label")?
        .as_str()
        .map(str::to_owned)
}

fn command_fields(descriptors: &[domain::FieldDescriptor]) -> Vec<Value> {
    descriptors
        .iter()
        .map(|descriptor| {
            let mut value = field_kind(descriptor.value.kind);
            for wrapper in descriptor.value.wrappers.iter().rev() {
                value = match wrapper {
                    domain::FieldWrapper::List => json!({ "kind": "list", "element": value }),
                    domain::FieldWrapper::Optional => {
                        json!({ "kind": "optional", "value": value })
                    }
                };
            }
            json!({ "name": descriptor.name, "value": value })
        })
        .collect()
}

fn field_kind(kind: domain::FieldKind) -> Value {
    match kind {
        domain::FieldKind::Scalar(scalar) => {
            json!({ "kind": "scalar", "scalar": scalar_name(scalar) })
        }
        domain::FieldKind::SemanticScalar(descriptor) => json!({
            "kind": "scalar",
            "scalar": {
                "kind": "semantic",
                "id": descriptor.id,
                "label": descriptor.label,
                "representation": scalar_name(descriptor.representation),
            }
        }),
        domain::FieldKind::Entity(id) => json!({
            "kind": "entity",
            "id": {
                "aggregate": {
                    "context": id.aggregate.context.0,
                    "local": id.aggregate.local,
                },
                "local": id.local,
            }
        }),
        domain::FieldKind::AggregateReference(id) => json!({
            "kind": "aggregateReference",
            "aggregate": { "context": id.context.0, "local": id.local }
        }),
        _ => json!({ "kind": "opaque" }),
    }
}

const fn scalar_name(scalar: domain::ScalarType) -> &'static str {
    match scalar {
        domain::ScalarType::Bool => "bool",
        domain::ScalarType::String => "string",
        domain::ScalarType::Char => "char",
        domain::ScalarType::F32 => "f32",
        domain::ScalarType::F64 => "f64",
        domain::ScalarType::I8 => "i8",
        domain::ScalarType::I16 => "i16",
        domain::ScalarType::I32 => "i32",
        domain::ScalarType::I64 => "i64",
        domain::ScalarType::I128 => "i128",
        domain::ScalarType::Isize => "isize",
        domain::ScalarType::U8 => "u8",
        domain::ScalarType::U16 => "u16",
        domain::ScalarType::U32 => "u32",
        domain::ScalarType::U64 => "u64",
        domain::ScalarType::U128 => "u128",
        domain::ScalarType::Usize => "usize",
    }
}

fn payload_template(fields: &[Value]) -> Value {
    let mut payload = Map::new();
    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let value = field.get("value").map_or(Value::Null, value_template);
        payload.insert(name.to_owned(), value);
    }
    Value::Object(payload)
}

fn value_template(value: &Value) -> Value {
    match value.get("kind").and_then(Value::as_str) {
        Some("scalar") => scalar_template(value.get("scalar")),
        Some("list") => Value::Array(Vec::new()),
        _ => Value::Null,
    }
}

fn scalar_template(scalar: Option<&Value>) -> Value {
    let scalar = scalar.and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("representation").and_then(Value::as_str))
    });
    match scalar {
        Some("bool") => Value::Bool(false),
        Some("f32" | "f64") => Value::from(0.0),
        Some(
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
            | "usize",
        ) => Value::from(0),
        Some("string" | "char") => Value::String(String::new()),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::value_template;

    #[test]
    fn identity_payload_templates_do_not_invent_a_scalar_representation() {
        let identity = json!({
            "kind": "identity",
            "id": {
                "owner": {
                    "aggregate": { "context": "banking", "local": "account" },
                    "local": "account"
                }
            }
        });
        assert_eq!(value_template(&identity), Value::Null);
    }

    #[test]
    fn value_object_payload_templates_do_not_invent_a_shape() {
        let value_object = json!({
            "kind": "valueObject",
            "id": { "context": "banking", "local": "amount" }
        });

        assert_eq!(value_template(&value_object), Value::Null);
    }

    #[test]
    fn opaque_dto_payload_templates_do_not_invent_a_shape() {
        assert_eq!(value_template(&json!({ "kind": "opaque" })), Value::Null);
    }
}
