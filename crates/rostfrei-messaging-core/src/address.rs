use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    ContractError, ContractErrorKind,
    scope::{TrafficScope, validate_scope_segment},
};

pub const COMMAND_ADDRESS_CONVENTION: &str = "<application>[.test].command.<context>.<name>";
pub const COMMAND_RESPONSE_ADDRESS_CONVENTION: &str =
    "<application>[.test].command-response.<context>.<name>";
pub const INTEGRATION_EVENT_ADDRESS_CONVENTION: &str =
    "<application>[.test].integration.<context>.<name>";
pub const QUERY_ADDRESS_CONVENTION: &str = "<application>[.test].query.<context>.<name>";
pub const MAX_ADDRESS_BYTES: usize = 256;
pub const MAX_ADDRESS_SEGMENT_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AddressKind {
    Command,
    CommandResponse,
    IntegrationEvent,
    Query,
}

impl AddressKind {
    pub const fn segment(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::CommandResponse => "command-response",
            Self::IntegrationEvent => "integration",
            Self::Query => "query",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandAddress(String);

impl CommandAddress {
    pub fn new(application: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        Self::new_in_scope(application, TrafficScope::Normal, context, name)
    }

    pub fn new_in_scope(
        application: &str,
        traffic_scope: TrafficScope,
        context: &str,
        name: &str,
    ) -> Result<Self, ContractError> {
        build_address(
            AddressKind::Command,
            application,
            traffic_scope,
            context,
            name,
        )
        .map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::Command).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        segment(&self.0, 0)
    }

    pub fn traffic_scope(&self) -> TrafficScope {
        address_traffic_scope(&self.0)
    }

    pub fn context(&self) -> &str {
        address_segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        address_segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandResponseAddress(String);

impl CommandResponseAddress {
    pub fn new(application: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        Self::new_in_scope(application, TrafficScope::Normal, context, name)
    }

    pub fn new_in_scope(
        application: &str,
        traffic_scope: TrafficScope,
        context: &str,
        name: &str,
    ) -> Result<Self, ContractError> {
        build_address(
            AddressKind::CommandResponse,
            application,
            traffic_scope,
            context,
            name,
        )
        .map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::CommandResponse).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        segment(&self.0, 0)
    }

    pub fn traffic_scope(&self) -> TrafficScope {
        address_traffic_scope(&self.0)
    }

    pub fn context(&self) -> &str {
        address_segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        address_segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IntegrationEventAddress(String);

impl IntegrationEventAddress {
    pub fn new(application: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        Self::new_in_scope(application, TrafficScope::Normal, context, name)
    }

    pub fn new_in_scope(
        application: &str,
        traffic_scope: TrafficScope,
        context: &str,
        name: &str,
    ) -> Result<Self, ContractError> {
        build_address(
            AddressKind::IntegrationEvent,
            application,
            traffic_scope,
            context,
            name,
        )
        .map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::IntegrationEvent).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        segment(&self.0, 0)
    }

    pub fn traffic_scope(&self) -> TrafficScope {
        address_traffic_scope(&self.0)
    }

    pub fn context(&self) -> &str {
        address_segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        address_segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct QueryAddress(String);

impl QueryAddress {
    pub fn new(application: &str, context: &str, name: &str) -> Result<Self, ContractError> {
        Self::new_in_scope(application, TrafficScope::Normal, context, name)
    }

    pub fn new_in_scope(
        application: &str,
        traffic_scope: TrafficScope,
        context: &str,
        name: &str,
    ) -> Result<Self, ContractError> {
        build_address(
            AddressKind::Query,
            application,
            traffic_scope,
            context,
            name,
        )
        .map(Self)
    }

    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        parse_address(value.into(), AddressKind::Query).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn application(&self) -> &str {
        segment(&self.0, 0)
    }

    pub fn traffic_scope(&self) -> TrafficScope {
        address_traffic_scope(&self.0)
    }

    pub fn context(&self) -> &str {
        address_segment(&self.0, 2)
    }

    pub fn name(&self) -> &str {
        address_segment(&self.0, 3)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum MessageAddress {
    Command(CommandAddress),
    CommandResponse(CommandResponseAddress),
    IntegrationEvent(IntegrationEventAddress),
    Query(QueryAddress),
}

impl MessageAddress {
    pub fn parse(value: impl Into<String>) -> Result<Self, ContractError> {
        let value = value.into();
        match parse_address_parts(&value)?.kind {
            AddressKind::Command => Ok(Self::Command(CommandAddress(value))),
            AddressKind::CommandResponse => {
                Ok(Self::CommandResponse(CommandResponseAddress(value)))
            }
            AddressKind::IntegrationEvent => {
                Ok(Self::IntegrationEvent(IntegrationEventAddress(value)))
            }
            AddressKind::Query => Ok(Self::Query(QueryAddress(value))),
        }
    }

    pub const fn kind(&self) -> AddressKind {
        match self {
            Self::Command(_) => AddressKind::Command,
            Self::CommandResponse(_) => AddressKind::CommandResponse,
            Self::IntegrationEvent(_) => AddressKind::IntegrationEvent,
            Self::Query(_) => AddressKind::Query,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Command(address) => address.as_str(),
            Self::CommandResponse(address) => address.as_str(),
            Self::IntegrationEvent(address) => address.as_str(),
            Self::Query(address) => address.as_str(),
        }
    }

    pub fn application(&self) -> &str {
        match self {
            Self::Command(address) => address.application(),
            Self::CommandResponse(address) => address.application(),
            Self::IntegrationEvent(address) => address.application(),
            Self::Query(address) => address.application(),
        }
    }

    pub fn context(&self) -> &str {
        match self {
            Self::Command(address) => address.context(),
            Self::CommandResponse(address) => address.context(),
            Self::IntegrationEvent(address) => address.context(),
            Self::Query(address) => address.context(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Command(address) => address.name(),
            Self::CommandResponse(address) => address.name(),
            Self::IntegrationEvent(address) => address.name(),
            Self::Query(address) => address.name(),
        }
    }

    pub fn traffic_scope(&self) -> TrafficScope {
        match self {
            Self::Command(address) => address.traffic_scope(),
            Self::CommandResponse(address) => address.traffic_scope(),
            Self::IntegrationEvent(address) => address.traffic_scope(),
            Self::Query(address) => address.traffic_scope(),
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
    fn application(&self) -> &str;
    fn traffic_scope(&self) -> TrafficScope;
    fn context(&self) -> &str;
    fn kind(&self) -> AddressKind;
}

impl private::Sealed for CommandAddress {}
impl private::Sealed for CommandResponseAddress {}
impl private::Sealed for IntegrationEventAddress {}

impl PublishableAddress for CommandAddress {
    fn as_str(&self) -> &str {
        self.as_str()
    }

    fn application(&self) -> &str {
        self.application()
    }

    fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope()
    }

    fn context(&self) -> &str {
        self.context()
    }

    fn kind(&self) -> AddressKind {
        AddressKind::Command
    }
}

impl PublishableAddress for CommandResponseAddress {
    fn as_str(&self) -> &str {
        self.as_str()
    }

    fn application(&self) -> &str {
        self.application()
    }

    fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope()
    }

    fn context(&self) -> &str {
        self.context()
    }

    fn kind(&self) -> AddressKind {
        AddressKind::CommandResponse
    }
}

impl PublishableAddress for IntegrationEventAddress {
    fn as_str(&self) -> &str {
        self.as_str()
    }

    fn application(&self) -> &str {
        self.application()
    }

    fn traffic_scope(&self) -> TrafficScope {
        self.traffic_scope()
    }

    fn context(&self) -> &str {
        self.context()
    }

    fn kind(&self) -> AddressKind {
        AddressKind::IntegrationEvent
    }
}

fn build_address(
    kind: AddressKind,
    application: &str,
    traffic_scope: TrafficScope,
    context: &str,
    name: &str,
) -> Result<String, ContractError> {
    validate_scope_segment(application, "address application")?;
    validate_scope_segment(context, "address context")?;
    validate_address_name(name)?;
    let value = traffic_scope.subject_segment().map_or_else(
        || format!("{application}.{}.{context}.{name}", kind.segment()),
        |scope| format!("{application}.{scope}.{}.{context}.{name}", kind.segment()),
    );
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
    let parts = parse_address_parts(&value)?;
    if parts.kind != expected {
        return Err(ContractError::new(
            ContractErrorKind::WrongAddressKind,
            "address",
        ));
    }
    Ok(value)
}

struct AddressParts {
    kind: AddressKind,
}

fn parse_address_parts(value: &str) -> Result<AddressParts, ContractError> {
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
    let Some(application) = segments.next() else {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "address",
        ));
    };
    let Some(second) = segments.next() else {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "address",
        ));
    };
    let kind = if second == "test" {
        segments.next()
    } else {
        Some(second)
    };
    let context = segments.next();
    let name = segments.next();
    if segments.next().is_some() || kind.is_none() || context.is_none() || name.is_none() {
        return Err(ContractError::new(
            ContractErrorKind::InvalidFormat,
            "address",
        ));
    }
    let kind = match kind.unwrap_or_default() {
        "command" => AddressKind::Command,
        "command-response" => AddressKind::CommandResponse,
        "integration" => AddressKind::IntegrationEvent,
        "query" => AddressKind::Query,
        _ => {
            return Err(ContractError::new(
                ContractErrorKind::WrongAddressKind,
                "address",
            ));
        }
    };
    validate_scope_segment(application, "address application")?;
    validate_scope_segment(context.unwrap_or_default(), "address context")?;
    validate_address_name(name.unwrap_or_default())?;
    Ok(AddressParts { kind })
}

fn validate_address_name(value: &str) -> Result<(), ContractError> {
    let field = "address name";
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

fn address_traffic_scope(value: &str) -> TrafficScope {
    if segment(value, 1) == "test" {
        TrafficScope::Test
    } else {
        TrafficScope::Normal
    }
}

fn address_segment(value: &str, normal_index: usize) -> &str {
    let index = if address_traffic_scope(value) == TrafficScope::Test && normal_index > 0 {
        normal_index.saturating_add(1)
    } else {
        normal_index
    };
    segment(value, index)
}

impl From<CommandAddress> for MessageAddress {
    fn from(value: CommandAddress) -> Self {
        Self::Command(value)
    }
}

impl From<CommandResponseAddress> for MessageAddress {
    fn from(value: CommandResponseAddress) -> Self {
        Self::CommandResponse(value)
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

impl fmt::Display for CommandResponseAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CommandResponseAddress {
    type Err = ContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for CommandResponseAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CommandResponseAddress {
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
        let command_response =
            CommandResponseAddress::new("acme", "orders", "a".repeat(64).as_str()).unwrap();
        let event = IntegrationEventAddress::new("acme", "orders", "order-placed").unwrap();
        let query = QueryAddress::new("acme", "orders", "find-order").unwrap();

        assert_eq!(command.as_str(), "acme.command.orders.place-order");
        assert_eq!(
            command_response.as_str(),
            format!("acme.command-response.orders.{}", "a".repeat(64))
        );
        assert_eq!(event.as_str(), "acme.integration.orders.order-placed");
        assert_eq!(query.as_str(), "acme.query.orders.find-order");
        assert_eq!(query.application(), "acme");
        assert_eq!(query.context(), "orders");
        assert_eq!(query.name(), "find-order");
    }

    #[test]
    fn public_parsing_rejects_kinds_wildcards_controls_and_bad_segments() {
        assert_eq!(
            MessageAddress::parse("acme.query.orders.find-order")
                .unwrap()
                .kind(),
            AddressKind::Query
        );
        assert_eq!(
            MessageAddress::parse(format!("acme.command-response.orders.{}", "a".repeat(64)))
                .unwrap()
                .kind(),
            AddressKind::CommandResponse
        );
        assert_eq!(
            CommandAddress::parse("acme.query.orders.find-order")
                .unwrap_err()
                .kind(),
            ContractErrorKind::WrongAddressKind
        );

        for invalid in [
            "acme.command.orders.*",
            "acme.command.orders.>",
            "acme.command.orders.bad\nname",
            "acme.command.orders",
            "acme.command.orders.too.many",
            "Acme.command.orders.place-order",
            "acme.command.orders.-place-order",
        ] {
            assert!(MessageAddress::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn test_addresses_add_one_validated_scope_segment() {
        let command =
            CommandAddress::new_in_scope("acme", TrafficScope::Test, "orders", "place-order")
                .unwrap();

        assert_eq!(command.as_str(), "acme.test.command.orders.place-order");
        assert_eq!(command.application(), "acme");
        assert_eq!(command.traffic_scope(), TrafficScope::Test);
        assert_eq!(command.context(), "orders");
        assert_eq!(command.name(), "place-order");
        assert_eq!(
            MessageAddress::parse(command.as_str())
                .unwrap()
                .traffic_scope(),
            TrafficScope::Test
        );
        assert!(MessageAddress::parse("acme.dev.command.orders.place-order").is_err());
    }

    #[test]
    fn address_deserialization_revalidates_input() {
        let parsed: QueryAddress =
            serde_json::from_str("\"acme.query.orders.find-order\"").unwrap();
        assert_eq!(parsed.name(), "find-order");
        assert!(serde_json::from_str::<QueryAddress>("\"acme.query.orders.*\"").is_err());
        let response: CommandResponseAddress = serde_json::from_str(&format!(
            "\"acme.command-response.orders.{}\"",
            "a".repeat(64)
        ))
        .unwrap();
        assert_eq!(response.name().len(), 64);
    }
}
