use rostfrei_tracer::{message_series_definition_schema, observed_message_series_schema};
use schemars::Schema;

#[test]
fn checked_in_message_series_schemas_match_the_rust_contracts() {
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

fn render(schema: &Schema) -> Result<String, serde_json::Error> {
    Ok(format!("{}\n", serde_json::to_string_pretty(schema)?))
}
