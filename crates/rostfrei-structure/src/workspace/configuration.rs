use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct StructureConfiguration {
    pub(super) version: u64,
    pub(super) domain_root: PathBuf,
}

pub(super) fn structure_configuration(
    metadata: &Value,
) -> Option<Result<StructureConfiguration, String>> {
    let structure = metadata.get("rostfrei")?.get("structure")?.clone();
    Some(serde_json::from_value(structure).map_err(|error| error.to_string()))
}
