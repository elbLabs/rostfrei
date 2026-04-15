use serde_json::Value;

use std::convert::Infallible;

use crate::DomainCommandType;

/// A domain command with a generated conventional JSON object representation.
pub trait JsonCommandPayload: DomainCommandType + Sized {
    fn decode_json(payload: &Value) -> Result<Self, String>;
}

/// A domain error with a generated conventional JSON object representation.
pub trait JsonErrorPayload {
    fn encode_json(&self) -> Result<Value, String>;
}

impl JsonErrorPayload for Infallible {
    fn encode_json(&self) -> Result<Value, String> {
        match *self {}
    }
}
