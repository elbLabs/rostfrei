use std::{error::Error, fs, path::Path};

use rostfrei_tracer::{
    behavioral_test_definition_schema, message_series_definition_schema,
    observed_message_series_schema,
};
use schemars::Schema;

fn main() -> Result<(), Box<dyn Error>> {
    let schema_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    write_schema(
        &schema_directory.join("behavioral-test-v1.schema.json"),
        &behavioral_test_definition_schema(),
    )?;
    write_schema(
        &schema_directory.join("message-series-definition-v1.schema.json"),
        &message_series_definition_schema(),
    )?;
    write_schema(
        &schema_directory.join("observed-message-series-v1.schema.json"),
        &observed_message_series_schema(),
    )?;
    Ok(())
}

fn write_schema(path: &Path, schema: &Schema) -> Result<(), Box<dyn Error>> {
    let schema = serde_json::to_value(schema)?;
    let document = serde_json::to_string_pretty(&schema)?;
    fs::write(path, format!("{document}\n"))?;
    Ok(())
}
