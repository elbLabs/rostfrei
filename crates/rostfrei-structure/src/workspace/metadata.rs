use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use crate::compiled_check::CargoTarget;

use super::{CheckError, CheckOptions};

#[derive(Debug, Deserialize)]
pub(super) struct Metadata {
    pub(super) packages: Vec<Package>,
    pub(super) workspace_members: Vec<String>,
    pub(super) workspace_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub(super) struct Package {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) manifest_path: PathBuf,
    pub(super) metadata: Value,
    pub(super) targets: Vec<CargoTarget>,
}

pub(super) fn load_metadata(options: &CheckOptions) -> Result<Metadata, CheckError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.args(["metadata", "--format-version", "1", "--no-deps"]);
    if let Some(manifest_path) = &options.manifest_path {
        command.arg("--manifest-path").arg(manifest_path);
    }
    let output = command.output().map_err(CheckError::MetadataInvocation)?;
    if !output.status.success() {
        return Err(CheckError::MetadataFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(CheckError::InvalidMetadata)
}
