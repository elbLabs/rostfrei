use std::any::type_name;
use std::collections::BTreeMap;

use rostfrei_core::{Aggregate, CommandHandler};
use rostfrei_messaging_core::{CommandAddress, QueryAddress};
use thiserror::Error;

const DIRECT_QUERY_REGISTRATION: &str = "<direct query registration>";

/// Connects a bounded-context command to its application-layer handler.
pub trait CommandDefinition<H>: domain::Command + Sized + Send + Sync + 'static
where
    H: CommandHandler<Self>,
{
    fn descriptor() -> CommandDescriptor {
        CommandDescriptor {
            bounded_context: <Self::Context as domain::BoundedContextType>::DESCRIPTOR
                .id
                .0,
            command_name: Self::LOCAL_ID,
            schema_version: Self::SCHEMA_VERSION,
            rust_command_type: type_name::<Self>(),
            rust_handler_type: type_name::<H>(),
            modeled_command: Self::DESCRIPTOR,
        }
    }
}

impl<H, C> CommandDefinition<H> for C
where
    H: CommandHandler<C>,
    C: domain::Command + Sized + Send + Sync + 'static,
{
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CommandIdentity {
    pub bounded_context: &'static str,
    pub command_name: &'static str,
    pub schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub bounded_context: &'static str,
    pub command_name: &'static str,
    pub schema_version: u32,
    pub rust_command_type: &'static str,
    pub rust_handler_type: &'static str,
    pub modeled_command: domain::CommandDescriptor,
}

impl CommandDescriptor {
    pub const fn identity(&self) -> CommandIdentity {
        CommandIdentity {
            bounded_context: self.bounded_context,
            command_name: self.command_name,
            schema_version: self.schema_version,
        }
    }

    pub const fn modeled_command(&self) -> &domain::CommandDescriptor {
        &self.modeled_command
    }
}

pub trait QueryDefinition: Sized + Send + Sync + 'static {
    type Response: Send + Sync + 'static;

    const BOUNDED_CONTEXT: &'static str;
    const QUERY_NAME: &'static str;
    const SCHEMA_VERSION: u32;

    fn descriptor() -> QueryDescriptor {
        QueryDescriptor {
            bounded_context: Self::BOUNDED_CONTEXT,
            query_name: Self::QUERY_NAME,
            schema_version: Self::SCHEMA_VERSION,
            rust_request_type: type_name::<Self>(),
            rust_response_type: type_name::<Self::Response>(),
            modeled_query: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct QueryIdentity {
    pub bounded_context: &'static str,
    pub query_name: &'static str,
    pub schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryDescriptor {
    pub bounded_context: &'static str,
    pub query_name: &'static str,
    pub schema_version: u32,
    pub rust_request_type: &'static str,
    pub rust_response_type: &'static str,
    pub modeled_query: Option<domain::QueryDescriptor>,
}

impl QueryDescriptor {
    pub const fn identity(&self) -> QueryIdentity {
        QueryIdentity {
            bounded_context: self.bounded_context,
            query_name: self.query_name,
            schema_version: self.schema_version,
        }
    }

    pub const fn modeled_query(&self) -> Option<&domain::QueryDescriptor> {
        self.modeled_query.as_ref()
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RegistrationError {
    #[error("aggregate type must not be empty ({rust_aggregate_type})")]
    EmptyAggregateType { rust_aggregate_type: &'static str },
    #[error("aggregate type `{aggregate_type}` has an invalid routing identity: {reason}")]
    InvalidAggregateType {
        aggregate_type: String,
        rust_aggregate_type: &'static str,
        reason: String,
    },
    #[error(
        "aggregate type `{aggregate_type}` from `{attempted_rust_aggregate_type}` is already registered by `{existing_rust_aggregate_type}`"
    )]
    DuplicateAggregateType {
        aggregate_type: String,
        existing_rust_aggregate_type: &'static str,
        attempted_rust_aggregate_type: &'static str,
    },
    #[error("command name must not be empty ({rust_command_type})")]
    EmptyCommandName { rust_command_type: &'static str },
    #[error("command `{command_name}` has schema version zero")]
    ZeroSchemaVersion { command_name: &'static str },
    #[error("command `{command_name}` version {schema_version} has an empty bounded context")]
    EmptyCommandBoundedContext {
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "command `{command_name}` version {schema_version} has an invalid routing identity: {reason}"
    )]
    InvalidCommandIdentity {
        command_name: &'static str,
        schema_version: u32,
        reason: String,
    },
    #[error(
        "command `{command_name}` version {schema_version} is already registered in bounded context `{bounded_context}`"
    )]
    DuplicateCommandIdentity {
        bounded_context: &'static str,
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "registration `{module_name}` contains a query with an empty bounded context ({rust_request_type})"
    )]
    EmptyQueryBoundedContext {
        module_name: &'static str,
        rust_request_type: &'static str,
    },
    #[error(
        "registration `{module_name}` contains a query with an empty name ({rust_request_type})"
    )]
    EmptyQueryName {
        module_name: &'static str,
        rust_request_type: &'static str,
    },
    #[error("query `{query_name}` in registration `{module_name}` has schema version zero")]
    ZeroQuerySchemaVersion {
        module_name: &'static str,
        query_name: &'static str,
    },
    #[error(
        "query `{query_name}` version {schema_version} in registration `{module_name}` has an invalid routing identity: {reason}"
    )]
    InvalidQueryIdentity {
        module_name: &'static str,
        query_name: &'static str,
        schema_version: u32,
        reason: String,
    },
    #[error(
        "query `{query_name}` version {schema_version} from registration `{attempted_module_name}` is already registered by registration `{existing_module_name}`"
    )]
    DuplicateQueryIdentityAcrossModules {
        query_name: &'static str,
        schema_version: u32,
        existing_module_name: &'static str,
        attempted_module_name: &'static str,
    },
}

#[derive(Debug)]
struct RegisteredAggregate {
    rust_aggregate_type: &'static str,
}

#[derive(Debug)]
struct RegisteredQuery {
    module_name: &'static str,
    descriptor: QueryDescriptor,
}

#[derive(Debug, Default)]
pub struct DomainRegistry {
    aggregates: BTreeMap<String, RegisteredAggregate>,
    commands: BTreeMap<&'static str, BTreeMap<&'static str, BTreeMap<u32, CommandDescriptor>>>,
    queries: BTreeMap<&'static str, BTreeMap<&'static str, BTreeMap<u32, RegisteredQuery>>>,
}

fn validate_aggregate_type(
    aggregate_type: &str,
    rust_aggregate_type: &'static str,
) -> Result<(), RegistrationError> {
    if aggregate_type.trim().is_empty() {
        return Err(RegistrationError::EmptyAggregateType {
            rust_aggregate_type,
        });
    }
    let (bounded_context, aggregate) = match aggregate_type.split_once('/') {
        Some((bounded_context, aggregate)) if !aggregate.contains('/') => {
            (bounded_context, aggregate)
        }
        Some(_) | None => {
            return Err(RegistrationError::InvalidAggregateType {
                aggregate_type: aggregate_type.to_owned(),
                rust_aggregate_type,
                reason: "aggregate type must be bounded-context-qualified with exactly one context separator"
                    .to_owned(),
            });
        }
    };
    if let Err(error) = CommandAddress::new("rostfrei", bounded_context, aggregate) {
        return Err(RegistrationError::InvalidAggregateType {
            aggregate_type: aggregate_type.to_owned(),
            rust_aggregate_type,
            reason: error.to_string(),
        });
    }
    Ok(())
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_aggregate<A>(&mut self) -> Result<(), RegistrationError>
    where
        A: Aggregate,
    {
        let aggregate_type = A::aggregate_type().into_owned();
        let rust_aggregate_type = type_name::<A>();
        validate_aggregate_type(&aggregate_type, rust_aggregate_type)?;

        if let Some(existing) = self.aggregates.get(&aggregate_type) {
            if existing.rust_aggregate_type == rust_aggregate_type {
                return Ok(());
            }
            return Err(RegistrationError::DuplicateAggregateType {
                aggregate_type,
                existing_rust_aggregate_type: existing.rust_aggregate_type,
                attempted_rust_aggregate_type: rust_aggregate_type,
            });
        }

        self.aggregates.insert(
            aggregate_type,
            RegisteredAggregate {
                rust_aggregate_type,
            },
        );
        Ok(())
    }

    pub fn register_command<C, H>(&mut self) -> Result<(), RegistrationError>
    where
        C: CommandDefinition<H>,
        H: CommandHandler<C>,
    {
        let command = <C as CommandDefinition<H>>::descriptor();
        self.validate_command(&command)?;
        self.insert_command(command);
        Ok(())
    }

    pub fn register_query<Q: QueryDefinition>(&mut self) -> Result<(), RegistrationError> {
        let query = Q::descriptor();
        self.validate_query(&query)?;
        self.insert_query(query);
        Ok(())
    }

    pub fn aggregates(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.aggregates.keys().map(String::as_str)
    }

    pub fn commands(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands
            .values()
            .flat_map(BTreeMap::values)
            .flat_map(BTreeMap::values)
    }

    pub fn command(
        &self,
        bounded_context: &str,
        command_name: &str,
        schema_version: u32,
    ) -> Option<&CommandDescriptor> {
        self.commands
            .get(bounded_context)
            .and_then(|commands| commands.get(command_name))
            .and_then(|versions| versions.get(&schema_version))
    }

    pub fn commands_for_context<'a>(
        &'a self,
        bounded_context: &'a str,
    ) -> impl Iterator<Item = &'a CommandDescriptor> + 'a {
        self.commands()
            .filter(move |command| command.bounded_context == bounded_context)
    }

    pub fn queries(&self) -> impl Iterator<Item = &QueryDescriptor> {
        self.queries
            .values()
            .flat_map(BTreeMap::values)
            .flat_map(BTreeMap::values)
            .map(|registered| &registered.descriptor)
    }

    pub fn query(
        &self,
        bounded_context: &str,
        query_name: &str,
        schema_version: u32,
    ) -> Option<&QueryDescriptor> {
        self.queries
            .get(bounded_context)
            .and_then(|queries| queries.get(query_name))
            .and_then(|versions| versions.get(&schema_version))
            .map(|registered| &registered.descriptor)
    }

    pub fn queries_for_context<'a>(
        &'a self,
        bounded_context: &'a str,
    ) -> impl Iterator<Item = &'a QueryDescriptor> + 'a {
        self.queries()
            .filter(move |query| query.bounded_context == bounded_context)
    }

    fn validate_command(&self, command: &CommandDescriptor) -> Result<(), RegistrationError> {
        if command.command_name.trim().is_empty() {
            return Err(RegistrationError::EmptyCommandName {
                rust_command_type: command.rust_command_type,
            });
        }
        if command.schema_version == 0 {
            return Err(RegistrationError::ZeroSchemaVersion {
                command_name: command.command_name,
            });
        }
        if command.bounded_context.trim().is_empty() {
            return Err(RegistrationError::EmptyCommandBoundedContext {
                command_name: command.command_name,
                schema_version: command.schema_version,
            });
        }
        if let Err(error) =
            CommandAddress::new("rostfrei", command.bounded_context, command.command_name)
        {
            return Err(RegistrationError::InvalidCommandIdentity {
                command_name: command.command_name,
                schema_version: command.schema_version,
                reason: error.to_string(),
            });
        }
        if self
            .command(
                command.bounded_context,
                command.command_name,
                command.schema_version,
            )
            .is_some()
        {
            return Err(RegistrationError::DuplicateCommandIdentity {
                bounded_context: command.bounded_context,
                command_name: command.command_name,
                schema_version: command.schema_version,
            });
        }
        Ok(())
    }

    fn validate_query(&self, query: &QueryDescriptor) -> Result<(), RegistrationError> {
        if query.bounded_context.trim().is_empty() {
            return Err(RegistrationError::EmptyQueryBoundedContext {
                module_name: DIRECT_QUERY_REGISTRATION,
                rust_request_type: query.rust_request_type,
            });
        }
        if query.query_name.trim().is_empty() {
            return Err(RegistrationError::EmptyQueryName {
                module_name: DIRECT_QUERY_REGISTRATION,
                rust_request_type: query.rust_request_type,
            });
        }
        if query.schema_version == 0 {
            return Err(RegistrationError::ZeroQuerySchemaVersion {
                module_name: DIRECT_QUERY_REGISTRATION,
                query_name: query.query_name,
            });
        }
        if let Err(error) = QueryAddress::new("rostfrei", query.bounded_context, query.query_name) {
            return Err(RegistrationError::InvalidQueryIdentity {
                module_name: DIRECT_QUERY_REGISTRATION,
                query_name: query.query_name,
                schema_version: query.schema_version,
                reason: error.to_string(),
            });
        }
        if let Some(existing) = self
            .queries
            .get(query.bounded_context)
            .and_then(|queries| queries.get(query.query_name))
            .and_then(|versions| versions.get(&query.schema_version))
        {
            return Err(RegistrationError::DuplicateQueryIdentityAcrossModules {
                query_name: query.query_name,
                schema_version: query.schema_version,
                existing_module_name: existing.module_name,
                attempted_module_name: DIRECT_QUERY_REGISTRATION,
            });
        }
        Ok(())
    }

    fn insert_command(&mut self, command: CommandDescriptor) {
        self.commands
            .entry(command.bounded_context)
            .or_default()
            .entry(command.command_name)
            .or_default()
            .insert(command.schema_version, command);
    }

    fn insert_query(&mut self, query: QueryDescriptor) {
        self.queries
            .entry(query.bounded_context)
            .or_default()
            .entry(query.query_name)
            .or_default()
            .insert(
                query.schema_version,
                RegisteredQuery {
                    module_name: DIRECT_QUERY_REGISTRATION,
                    descriptor: query,
                },
            );
    }
}
