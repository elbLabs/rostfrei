use std::collections::BTreeMap;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::{ContractError, ContractErrorKind};

pub const MAX_METADATA_ENTRIES: usize = 32;
pub const MAX_METADATA_NAME_BYTES: usize = 64;
pub const MAX_METADATA_VALUE_BYTES: usize = 1024;
pub const MAX_METADATA_BYTES: usize = 8192;
pub const MAX_TRACE_STATE_BYTES: usize = 512;

const RESERVED_NAMES: &[&str] = &[
    "content-type",
    "reply",
    "reply-to",
    "reply-subject",
    "traceparent",
    "tracestate",
];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CallerMetadata {
    entries: BTreeMap<String, String>,
}

impl CallerMetadata {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn try_from_entries<I, K, V>(entries: I) -> Result<Self, ContractError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut metadata = Self::new();
        for (name, value) in entries {
            metadata.insert(name, value)?;
        }
        Ok(metadata)
    }

    pub fn insert(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>, ContractError> {
        let name = validate_name(name.into())?;
        let value = validate_value(value.into())?;
        let existing = self.entries.get(&name);

        if existing.is_none() && self.entries.len() == MAX_METADATA_ENTRIES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooManyEntries,
                "caller metadata",
                self.entries.len() + 1,
                MAX_METADATA_ENTRIES,
            ));
        }

        let current_bytes = self.total_bytes();
        let replaced_bytes = existing.map_or(0, |old| name.len() + old.len());
        let projected_bytes = current_bytes - replaced_bytes + name.len() + value.len();
        if projected_bytes > MAX_METADATA_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooLong,
                "caller metadata",
                projected_bytes,
                MAX_METADATA_BYTES,
            ));
        }

        Ok(self.entries.insert(name, value))
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn remove(&mut self, name: &str) -> Option<String> {
        self.entries.remove(&name.to_ascii_lowercase())
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum()
    }
}

impl Serialize for CallerMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.entries.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CallerMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::try_from_entries(entries).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TraceContext {
    #[serde(rename = "traceparent")]
    trace_parent: String,
    #[serde(rename = "tracestate", skip_serializing_if = "Option::is_none")]
    trace_state: Option<String>,
}

impl TraceContext {
    pub fn new(trace_parent: impl Into<String>) -> Result<Self, ContractError> {
        Self::from_parts(trace_parent, None::<String>)
    }

    pub fn from_parts(
        trace_parent: impl Into<String>,
        trace_state: Option<impl Into<String>>,
    ) -> Result<Self, ContractError> {
        let trace_parent = trace_parent.into();
        validate_trace_parent(&trace_parent)?;
        let trace_state = trace_state.map(Into::into);
        if let Some(value) = &trace_state {
            validate_trace_state(value)?;
        }
        Ok(Self {
            trace_parent,
            trace_state,
        })
    }

    pub fn trace_parent(&self) -> &str {
        &self.trace_parent
    }

    pub fn trace_state(&self) -> Option<&str> {
        self.trace_state.as_deref()
    }
}

#[derive(Deserialize)]
struct TraceContextWire {
    #[serde(rename = "traceparent")]
    trace_parent: String,
    #[serde(rename = "tracestate")]
    trace_state: Option<String>,
}

impl<'de> Deserialize<'de> for TraceContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TraceContextWire::deserialize(deserializer)?;
        Self::from_parts(wire.trace_parent, wire.trace_state).map_err(D::Error::custom)
    }
}

fn validate_name(mut name: String) -> Result<String, ContractError> {
    if name.is_empty() {
        return Err(ContractError::new(
            ContractErrorKind::Empty,
            "metadata name",
        ));
    }
    if name.len() > MAX_METADATA_NAME_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            "metadata name",
            name.len(),
            MAX_METADATA_NAME_BYTES,
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            "metadata name",
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "metadata name",
        ));
    }

    name.make_ascii_lowercase();
    if name.starts_with("nats-")
        || name.starts_with("zeitstrahl-control-")
        || RESERVED_NAMES.contains(&name.as_str())
    {
        return Err(ContractError::new(
            ContractErrorKind::Reserved,
            "metadata name",
        ));
    }
    Ok(name)
}

fn validate_value(value: String) -> Result<String, ContractError> {
    if value.len() > MAX_METADATA_VALUE_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            "metadata value",
            value.len(),
            MAX_METADATA_VALUE_BYTES,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            "metadata value",
        ));
    }
    if !value.is_ascii() {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "metadata value",
        ));
    }
    Ok(value)
}

fn validate_trace_parent(value: &str) -> Result<(), ContractError> {
    let mut parts = value.split('-');
    let version = parts.next();
    let trace_id = parts.next();
    let parent_id = parts.next();
    let flags = parts.next();
    let exact_shape = parts.next().is_none()
        && version == Some("00")
        && trace_id.is_some_and(|part| valid_lower_hex(part, 32) && !all_zero(part))
        && parent_id.is_some_and(|part| valid_lower_hex(part, 16) && !all_zero(part))
        && flags.is_some_and(|part| valid_lower_hex(part, 2));
    if !exact_shape {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "traceparent",
        ));
    }
    Ok(())
}

fn validate_trace_state(value: &str) -> Result<(), ContractError> {
    if value.is_empty() || value.len() > MAX_TRACE_STATE_BYTES {
        return Err(ContractError::bounded(
            if value.is_empty() {
                ContractErrorKind::Empty
            } else {
                ContractErrorKind::TooLong
            },
            "tracestate",
            value.len(),
            MAX_TRACE_STATE_BYTES,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            "tracestate",
        ));
    }
    if !value.is_ascii() {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "tracestate",
        ));
    }
    let members = value.split(',').collect::<Vec<_>>();
    if members.len() > 32
        || members.iter().any(|member| {
            let member = member.trim();
            let Some((key, member_value)) = member.split_once('=') else {
                return true;
            };
            key.is_empty()
                || member_value.is_empty()
                || !key.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'*' | b'/' | b'@')
                })
        })
    {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "tracestate",
        ));
    }
    Ok(())
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn all_zero(value: &str) -> bool {
    value.bytes().all(|byte| byte == b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE_PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn caller_metadata_normalizes_names_and_rejects_adapter_controls() {
        let mut metadata = CallerMetadata::new();
        metadata.insert("X-Tenant", "acme").unwrap();
        assert_eq!(metadata.get("x-tenant"), Some("acme"));
        assert_eq!(metadata.get("X-TENANT"), Some("acme"));

        for reserved in [
            "Content-Type",
            "Reply-To",
            "Reply-Subject",
            "Nats-Msg-Id",
            "NATS-Expected-Stream",
            "traceparent",
            "zeitstrahl-control-test",
        ] {
            assert_eq!(
                metadata.insert(reserved, "value").unwrap_err().kind(),
                ContractErrorKind::Reserved
            );
        }
        assert!(metadata.insert("x-bad", "line\nbreak").is_err());
    }

    #[test]
    fn trace_context_validates_w3c_headers() {
        let trace = TraceContext::from_parts(TRACE_PARENT, Some("acme=opaque")).unwrap();
        assert_eq!(trace.trace_parent(), TRACE_PARENT);
        assert_eq!(trace.trace_state(), Some("acme=opaque"));

        assert!(
            TraceContext::new("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_err()
        );
        assert!(TraceContext::from_parts(TRACE_PARENT, Some("bad\nstate")).is_err());
    }

    #[test]
    fn metadata_deserialization_cannot_bypass_reserved_name_checks() {
        assert!(serde_json::from_str::<CallerMetadata>("{\"Nats-Msg-Id\":\"one\"}").is_err());
    }
}
