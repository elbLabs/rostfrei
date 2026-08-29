use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::{
    ConsumerName, ContractError, ContractErrorKind, DurableName,
    address::{CommandAddress, IntegrationEventAddress, QueryAddress},
};

pub const MAX_SCOPE_NAME_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ApplicationName(String);

impl ApplicationName {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_scope_name(value.into(), "application name").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn bounded_context(
        &self,
        name: impl Into<String>,
    ) -> Result<BoundedContext, ContractError> {
        Ok(BoundedContext {
            application: self.clone(),
            name: BoundedContextName::new(name)?,
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BoundedContextName(String);

impl BoundedContextName {
    pub fn new(value: impl Into<String>) -> Result<Self, ContractError> {
        validate_scope_name(value.into(), "bounded context name").map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BoundedContext {
    application: ApplicationName,
    name: BoundedContextName,
}

impl BoundedContext {
    pub const fn new(application: ApplicationName, name: BoundedContextName) -> Self {
        Self { application, name }
    }

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub const fn name(&self) -> &BoundedContextName {
        &self.name
    }

    pub fn command_address(&self, name: &str) -> Result<CommandAddress, ContractError> {
        CommandAddress::new(self.application.as_str(), self.name.as_str(), name)
    }

    pub fn integration_event_address(
        &self,
        name: &str,
    ) -> Result<IntegrationEventAddress, ContractError> {
        IntegrationEventAddress::new(self.application.as_str(), self.name.as_str(), name)
    }

    pub fn query_address(&self, name: &str) -> Result<QueryAddress, ContractError> {
        QueryAddress::new(self.application.as_str(), self.name.as_str(), name)
    }

    pub fn consumer_name(
        &self,
        purpose: &str,
        major_version: u32,
    ) -> Result<ConsumerName, ContractError> {
        ConsumerName::new(
            self.application.as_str(),
            self.name.as_str(),
            purpose,
            major_version,
        )
    }

    pub fn durable_name(
        &self,
        purpose: &str,
        major_version: u32,
    ) -> Result<DurableName, ContractError> {
        DurableName::new(
            self.application.as_str(),
            self.name.as_str(),
            purpose,
            major_version,
        )
    }
}

pub fn validate_scope_segment(value: &str, field: &'static str) -> Result<(), ContractError> {
    validate_scope_name(value.to_owned(), field).map(|_| ())
}

fn validate_scope_name(value: String, field: &'static str) -> Result<String, ContractError> {
    if value.is_empty() {
        return Err(ContractError::new(ContractErrorKind::Empty, field));
    }
    if value.len() > MAX_SCOPE_NAME_BYTES {
        return Err(ContractError::bounded(
            ContractErrorKind::TooLong,
            field,
            value.len(),
            MAX_SCOPE_NAME_BYTES,
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
    Ok(value)
}

macro_rules! impl_string_value {
    ($type:ty) => {
        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = ContractError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }
    };
}

impl_string_value!(ApplicationName);
impl_string_value!(BoundedContextName);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_binds_addresses_to_one_bounded_context() {
        let application = ApplicationName::new("fast-inbox").unwrap();
        let context = application.bounded_context("commercial-access").unwrap();

        assert_eq!(
            context.command_address("evaluate").unwrap().as_str(),
            "fast-inbox.command.commercial-access.evaluate"
        );
        assert_eq!(
            context
                .integration_event_address("entitlement-changed")
                .unwrap()
                .as_str(),
            "fast-inbox.integration.commercial-access.entitlement-changed"
        );
        assert_eq!(
            context
                .query_address("current-entitlement")
                .unwrap()
                .as_str(),
            "fast-inbox.query.commercial-access.current-entitlement"
        );
        assert_eq!(
            context.consumer_name("evaluate", 1).unwrap().as_str(),
            "fast-inbox--commercial-access--evaluate--v1"
        );
        assert_eq!(
            context.durable_name("evaluate", 1).unwrap().as_str(),
            "fast-inbox--commercial-access--evaluate--v1"
        );
    }

    #[test]
    fn scope_names_are_lowercase_kebab_case() {
        for invalid in ["", "Fast-Inbox", "fast_inbox", "-fast-inbox", "fast--inbox"] {
            assert!(ApplicationName::new(invalid).is_err(), "{invalid}");
            assert!(BoundedContextName::new(invalid).is_err(), "{invalid}");
        }
    }
}
