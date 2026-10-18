use serde::Deserialize;
use serde_json::Value;

use super::{ErrorKind, Phase, Result, ToolError, bundle_files};

// HTTP envelope from rostfrei-tracer::service::OperationMessageSeries. The
// nested v2 observations use the runtime's generated JSON Schema, not a second
// message model/engine. This wire format has no numeric schemaVersion field.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationSeries {
    operation_id: String,
    correlation_id: String,
    mode: Mode,
    message_series: Value,
    capture: Capture,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Simulate,
    Test,
    Dispatch,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Fidelity {
    Exact,
    Grouped,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Capture {
    settled: bool,
    settled_for: Value,
    fidelity: Fidelity,
    note: Option<String>,
}

pub(super) fn validate(value: &Value) -> Result<()> {
    let invalid = || {
        ToolError::new(
            ErrorKind::InvalidResponse,
            Phase::Capture,
            "Malformed or unsupported operation MessageSeries capture",
        )
    };
    let wire: OperationSeries = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if wire.operation_id.trim().is_empty() || wire.correlation_id.trim().is_empty() {
        return Err(invalid());
    }
    // Decoding verifies these wire types even when the capture is empty/partial.
    let _ = (
        wire.mode,
        wire.capture.settled,
        wire.capture.fidelity,
        wire.capture.note,
    );
    let observations: Value =
        serde_json::from_slice(bundle_files::OBSERVED_SCHEMA).map_err(|_| invalid())?;
    let behavior: Value =
        serde_json::from_slice(bundle_files::BEHAVIOR_SCHEMA).map_err(|_| invalid())?;
    let timeout = behavior
        .pointer("/$defs/BehavioralTimeoutSchema")
        .ok_or_else(invalid)?;
    for (schema, instance) in [
        (&observations, &wire.message_series),
        (timeout, &wire.capture.settled_for),
    ] {
        let validator = jsonschema::validator_for(schema).map_err(|_| invalid())?;
        if !validator.is_valid(instance) {
            return Err(invalid());
        }
    }
    Ok(())
}
