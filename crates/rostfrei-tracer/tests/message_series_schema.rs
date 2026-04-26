use rostfrei_tracer::{
    behavioral_test_definition_schema, message_series_definition_schema,
    observed_message_series_schema,
};
use schemars::Schema;

#[test]
fn checked_in_message_series_schemas_match_the_rust_contracts() {
    let behavioral = render(&behavioral_test_definition_schema());
    assert!(behavioral.is_ok(), "behavioral schema must serialize");
    assert_eq!(
        behavioral.unwrap_or_default(),
        include_str!("../schema/behavioral-test-v1.schema.json")
    );
    let definition = render(&message_series_definition_schema());
    assert!(definition.is_ok(), "definition schema must serialize");
    assert_eq!(
        definition.unwrap_or_default(),
        include_str!("../schema/message-series-definition-v1.schema.json")
    );
    let observation = render(&observed_message_series_schema());
    assert!(observation.is_ok(), "observation schema must serialize");
    assert_eq!(
        observation.unwrap_or_default(),
        include_str!("../schema/observed-message-series-v1.schema.json")
    );
}

#[test]
fn observed_schema_only_allows_null_values_for_accepted_outcomes() {
    let schema = serde_json::to_value(observed_message_series_schema());
    assert!(schema.is_ok(), "observation schema must serialize");
    let schema = schema.unwrap_or_default();
    assert_eq!(
        schema.pointer("/$defs/CommandResponseOutcomeSchema/oneOf/0/properties/value/type"),
        Some(&serde_json::json!("null"))
    );
}

#[test]
fn observed_domain_aggregate_schema_requires_complete_identity() {
    let schema = serde_json::to_value(observed_message_series_schema());
    assert!(schema.is_ok(), "observation schema must serialize");
    let schema = schema.unwrap_or_default();
    assert_eq!(
        schema.pointer("/$defs/ObservedMessageNode/oneOf/1/properties/aggregate/anyOf/0/$ref"),
        Some(&serde_json::json!("#/$defs/AggregateSchema"))
    );
    assert_eq!(
        schema.pointer("/$defs/AggregateSchema/required"),
        Some(&serde_json::json!(["type", "id"]))
    );
    assert!(
        schema
            .pointer("/$defs/ObservedMessageNode/oneOf/1/required")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|required| !required.contains(&serde_json::json!("aggregate")))
    );
}

#[test]
fn generated_unsigned_integers_have_explicit_maxima() {
    for document in [
        include_str!("../schema/behavioral-test-v1.schema.json"),
        include_str!("../schema/message-series-definition-v1.schema.json"),
        include_str!("../schema/observed-message-series-v1.schema.json"),
    ] {
        let schema = serde_json::from_str::<serde_json::Value>(document);
        assert!(schema.is_ok(), "checked-in schema must be valid JSON");
        assert_unsigned_integer_maxima(&schema.unwrap_or_default());
    }
}

#[test]
fn behavioral_schema_reserves_the_validation_route_identifier() {
    let schema = serde_json::to_value(behavioral_test_definition_schema());
    assert!(schema.is_ok(), "behavioral schema must serialize");
    assert_eq!(
        schema
            .unwrap_or_default()
            .pointer("/properties/id/not/const"),
        Some(&serde_json::json!("validate"))
    );
}

fn render(schema: &Schema) -> Result<String, serde_json::Error> {
    let schema = serde_json::to_value(schema)?;
    Ok(format!("{}\n", serde_json::to_string_pretty(&schema)?))
}

fn assert_unsigned_integer_maxima(schema: &serde_json::Value) {
    match schema {
        serde_json::Value::Object(object) => {
            if let Some(format @ ("uint32" | "uint64")) =
                object.get("format").and_then(serde_json::Value::as_str)
            {
                let expected = if format == "uint32" {
                    u64::from(u32::MAX)
                } else {
                    u64::MAX
                };
                assert!(
                    object
                        .get("maximum")
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|maximum| maximum <= expected),
                    "{format} schema must have an explicit in-range maximum"
                );
            }
            for value in object.values() {
                assert_unsigned_integer_maxima(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                assert_unsigned_integer_maxima(value);
            }
        }
        _ => {}
    }
}
