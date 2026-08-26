use std::{fmt, str::FromStr};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

use crate::{ContractError, ContractErrorKind};

pub const COMMAND_ADDRESS_CONVENTION: &str = "command.<owner>.<context>.<name>";
pub const INTEGRATION_EVENT_ADDRESS_CONVENTION: &str = "integration.<owner>.<context>.<name>";
pub const QUERY_ADDRESS_CONVENTION: &str = "query.<owner>.<context>.<name>";
pub const MAX_ADDRESS_BYTES: usize = 256;
pub const MAX_ADDRESS_SEGMENT_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AddressKind {
    Command,
    IntegrationEvent,
    Query,
}

impl AddressKind {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::IntegrationEvent => "integration",
            Self::Query => "query",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandAddress(String);

impl CommandAddress {
    pub fn new(owner: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        build_address(AddressKind::Command, owner, context, name).map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::Command).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn owner(&self) -> &str {
        segment(&self.0, 1)
    }

    pub fn context(&self) -> &str {
        segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IntegrationEventAddress(String);

impl IntegrationEventAddress {
    pub fn new(owner: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        build_address(AddressKind::IntegrationEvent, owner, context, name).map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::IntegrationEvent).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn owner(&self) -> &str {
        segment(&self.0, 1)
    }

    pub fn context(&self) -> &str {
        segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryAddress(String);

impl QueryAddress {
    pub fn new(owner: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        build_address(AddressKind::Query, owner, context, name).map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::Query).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn owner(&self) -> &str {
        segment(&self.0, 1)
    }

    pub fn context(&self) -> &str {
        segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MessageAddress {
    Command(CommandAddress),
    IntegrationEvent(IntegrationEventAddress),
    Query(QueryAddress),
}

impl MessageAddress {
    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ContractError::new(ContractErrorKind::Empty, "address"));
        }
        if value.len() > MAX_ADDRESS_BYTES {
            return Err(ContractError::bounded(
                ContractErrorKind::TooLong,
                "address",
                value.len(),
                MAX_ADDRESS_BYTES,
            ));
        }
        if value.contains('*') || value.contains('>') {
            return Err(ContractError::new(ContractErrorKind::Wildcard, "address"));
        }
        if value.chars().any(char::is_control) {
            return Err(ContractError::new(
                ContractErrorKind::ControlCharacter,
                "address",
            ));
        }
        let prefix = value.split('.').next().unwrap_or_default();
        match prefix {
            "command" => CommandAddress::parse(value).map(Self::Command),
            "integration" => IntegrationEventAddress::parse(value).map(Self::IntegrationEvent),
            "query" => QueryAddress::parse(value).map(Self::Query),
            _ => Err(ContractError::new(
                ContractErrorKind::WrongAddressKind,
                "address",
            )),
        }
    }

    pub const fn kind(&self) -> AddressKind {
        match self {
            Self::Command(_) => AddressKind::Command,
            Self::IntegrationEvent(_) => AddressKind::IntegrationEvent,
            Self::Query(_) => AddressKind::Query,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Command(address) => address.as_str(),
            Self::IntegrationEvent(address) => address.as_str(),
            Self::Query(address) => address.as_str(),
        }
    }
}

mod private {
    pub trait Sealed {}
}

pub trait PublishableAddress:
    private::Sealed + Clone + fmt::Debug + Eq + Send + Sync + 'static
{
    fn as_str(&self) -> &str;
    fn kind(&self) -> AddressKind;
}

impl private::Sealed for CommandAddress {}
impl private::Sealed for IntegrationEventAddress {}

impl PublishableAddress for CommandAddress {
    fn as_str(&self) -> &str {
        self.as_str()
    }

    fn kind(&self) -> AddressKind {
        AddressKind::Command
    }
}

impl PublishableAddress for IntegrationEventAddress {
    fn as_str(&self) -> &str {
        self.as_str()
    }

    fn kind(&self) -> AddressKind {
        AddressKind::IntegrationEvent
    }
}

fn build_address(
    kind: AddressKind,
    owner: &str,
    context: &str,
    name: &str,
) -> Result<String, ContractError> {
    for (field, value) in [
        ("address owner", owner),
        ("address context", context),
        ("address name", name),
    ] {
        validate_segment(value, field)?;
    }
    let value = format!("{}.{}.{}.{}", kind.prefix(), owner, context, name);
    if value.len() > MAX_ADDRESS_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            "address",
            value.len(),
            MAX_ADDRESS_BYTES,
        ));
    }
    Ok(value)
}

fn parse_address(value: String, expected: AddressKind) -> Result<String, ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, "address"));
    }
    if value.len() > MAX_ADDRESS_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            "address",
            value.len(),
            MAX_ADDRESS_BYTES,
        ));
    }
    if value.contains('*') || value.contains('>') {
        return Err(ContractError::new(ContractErrorKind::Wildcard, "address"));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            "address",
        ));
    }

    let mut segments = value.split('.');
    let prefix = segments.next();
    let owner = segments.next();
    let context = segments.next();
    let name = segments.next();
    if segments.next().is_some() || owner.is_none() || context.is_none() || name.is_none() {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "address",
        ));
    }
    if prefix != Some(expected.prefix()) {
        return Err(ContractError::new(
            ContractErrorKind::WrongAddressKind,
            "address",
        ));
    }
    validate_segment(owner.unwrap_or_default(), "address owner")?;
    validate_segment(context.unwrap_or_default(), "address context")?;
    validate_segment(name.unwrap_or_default(), "address name")?;
    Ok(value)
}

pub(crate) fn validate_segment(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, field));
    }
    if value.len() > MAX_ADDRESS_SEGMENT_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            MAX_ADDRESS_SEGMENT_BYTES,
        ));
    }
    if value.contains('*') || value.contains('>') {
        return Err(ContractError::new(ContractErrorKind::Wildcard, field));
    }
    if value.chars().any(char::is_control) {
        return Err(ContractError::new(
            ContractErrorKind::ControlCharacter,
            field,
        ));
    }
    if value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ContractError::new(ContractErrorKind::InvalidFormat, field));
    }
    Ok(())
}

fn segment(value: &str, index: usize) -> &str {
    value.split('.').nth(index).unwrap_or_default()
}

impl From<CommandAddress> for MessageAddress {
    fn from(value: CommandAddress) -> Self {
        Self::Command(value)
    }
}

impl From<IntegrationEventAddress> for MessageAddress {
    fn from(value: IntegrationEventAddress) -> Self {
        Self::IntegrationEvent(value)
    }
}

impl From<QueryAddress> for MessageAddress {
    fn from(value: QueryAddress) -> Self {
        Self::Query(value)
    }
}

impl fmt::Display for CommandAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CommandAddress {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for CommandAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CommandAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl fmt::Display for IntegrationEventAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for IntegrationEventAddress {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for IntegrationEventAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IntegrationEventAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl fmt::Display for QueryAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for QueryAddress {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for QueryAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for QueryAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl fmt::Display for MessageAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for MessageAddress {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for MessageAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MessageAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_addresses_follow_four_segment_conventions() {
        let command = CommandAddress::new("acme", "orders", "place-order").unwrap();
        let event = IntegrationEventAddress::new("acme", "orders", "order-placed").unwrap();
        let query = QueryAddress::new("acme", "orders", "find-order").unwrap();

        assert_eq!(command.as_str(), "command.acme.orders.place-order");
        assert_eq!(event.as_str(), "integration.acme.orders.order-placed");
        assert_eq!(query.as_str(), "query.acme.orders.find-order");
        assert_eq!(query.owner(), "acme");
        assert_eq!(query.context(), "orders");
        assert_eq!(query.name(), "find-order");
    }

    #[test]
    fn public_parsing_rejects_kinds_wildcards_controls_and_bad_segments() {
        assert_eq!(
            MessageAddress::parse("query.acme.orders.find-order")
                .unwrap()
                .kind(),
            AddressKind::Query
        );
        assert_eq!(
            CommandAddress::parse("query.acme.orders.find-order")
                .unwrap_err()
                .kind(),
            ContractErrorKind::WrongAddressKind
        );

        for invalid in [
            "command.acme.orders.*",
            "command.acme.orders.>",
            "command.acme.orders.bad\nname",
            "command.acme.orders",
            "command.acme.orders.too.many",
            "command.Acme.orders.place-order",
            "command.acme.orders.-place-order",
        ] {
            assert!(MessageAddress::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn address_deserialization_revalidates_input() {
        let parsed: QueryAddress =
            serde_json::from_str("\"query.acme.orders.find-order\"").unwrap();
        assert_eq!(parsed.name(), "find-order");
        assert!(serde_json::from_str::<QueryAddress>("\"query.acme.orders.*\"").is_err());
    }
}
