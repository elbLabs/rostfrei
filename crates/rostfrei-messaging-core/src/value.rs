use std::fmt;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::{ContractError, ContractErrorKind};

pub const MAX_IDENTIFIER_BYTES: usize = 256;
pub const MAX_UNIX_TIMESTAMP_MILLISECONDS: u64 = 253_402_300_799_999;

fn validate_identifier(value: String, field: &'static str) -> Result<String, ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, field));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            MAX_IDENTIFIER_BYTES,
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            field,
        ));
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    }
    Ok(value)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MessageId(String);

impl MessageId {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_identifier(value.into(), "message_id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for MessageId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MessageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(String);

impl OperationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_identifier(value.into(), "operation_id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for OperationId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OperationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl From<OperationId> for MessageId {
    fn from(value: OperationId) -> Self {
        Self(value.0)
    }
}

impl From<&OperationId> for MessageId {
    fn from(value: &OperationId) -> Self {
        Self(value.0.clone())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CorrelationId(String);

impl CorrelationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_identifier(value.into(), "correlation_id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for CorrelationId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CorrelationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CausationId(String);

impl CausationId {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_identifier(value.into(), "causation_id").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CausationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for CausationId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CausationId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl From<MessageId> for CausationId {
    fn from(value: MessageId) -> Self {
        Self(value.0)
    }
}

impl From<&MessageId> for CausationId {
    fn from(value: &MessageId) -> Self {
        Self(value.0.clone())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    pub fn new(value: u32) -> Result<Self, ContractError> {
        if value == 0 {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "schema_version",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MessageTimestamp(u64);

impl MessageTimestamp {
    pub fn from_unix_milliseconds(value: u64) -> Result<Self, ContractError> {
        if value > MAX_UNIX_TIMESTAMP_MILLISECONDS {
            return Err(ContractError::new(
                ContractErrorKind::OutOfRange,
                "timestamp",
            ));
        }
        Ok(Self(value))
    }

    pub const fn unix_milliseconds(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for MessageTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_unix_milliseconds(u64::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_distinct_bounded_visible_ascii_values() {
        assert_eq!(MessageId::new("message-1").unwrap().as_str(), "message-1");
        assert_eq!(
            OperationId::new("operation:1").unwrap().as_str(),
            "operation:1"
        );
        assert_eq!(
            CorrelationId::new("correlation/1").unwrap().as_str(),
            "correlation/1"
        );
        assert_eq!(CausationId::new("cause_1").unwrap().as_str(), "cause_1");

        for invalid in ["", "has space", "has\ncontrol", "non-ascii-é"] {
            assert!(MessageId::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn schema_versions_and_timestamps_reject_out_of_range_values() {
        assert!(SchemaVersion::new(0).is_err());
        assert_eq!(SchemaVersion::new(1).unwrap().get(), 1);
        assert!(
            MessageTimestamp::from_unix_milliseconds(MAX_UNIX_TIMESTAMP_MILLISECONDS + 1).is_err()
        );
    }

    #[test]
    fn identifier_deserialization_revalidates_input() {
        assert!(serde_json::from_str::<CorrelationId>("\"has space\"").is_err());
        assert!(serde_json::from_str::<SchemaVersion>("0").is_err());
    }
}
