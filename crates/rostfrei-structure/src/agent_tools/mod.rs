mod behavior;
mod http;
mod install;
mod release;

use serde::Serialize;
use serde_json::{Map, Value};
use thiserror::Error;

pub use behavior::{TestOptions, run_test};
pub use install::install_skills;
pub use release::Release;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    IncompleteResponse,
    RequestFailed,
    InvalidResponse,
    SetupFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparation,
    Configuration,
    Catalog,
    Schema,
    Validation,
    Fixture,
    Run,
    Capture,
    EvidenceFile,
    Installation,
}

/// Client-side failures, never an assertion verdict from the Tracer suite.
#[derive(Clone, Debug, Error, Serialize)]
#[error("{message}")]
pub struct ToolError {
    pub kind: ErrorKind,
    pub message: String,
    pub phase: Phase,
    pub details: Map<String, Value>,
}

impl ToolError {
    pub fn new(kind: ErrorKind, phase: Phase, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            phase,
            details: Map::new(),
        }
    }

    pub fn detail(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.details.insert(key.to_owned(), value.into());
        self
    }
}

type Result<T> = std::result::Result<T, ToolError>;

fn json_file(path: &std::path::Path, phase: Phase) -> Result<Value> {
    let bytes = std::fs::read(path)
        .map_err(|_| ToolError::new(ErrorKind::SetupFailed, phase, "Could not read JSON input"))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ToolError::new(ErrorKind::SetupFailed, phase, "Input is not valid JSON"))
}

fn field<'a>(value: &'a Value, pointer: &str, phase: Phase) -> Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ToolError::new(
                ErrorKind::InvalidResponse,
                phase,
                "Missing or invalid protocol field",
            )
        })
}
