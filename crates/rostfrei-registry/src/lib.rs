use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};

use rostfrei_core::{Aggregate, CommandHandler};
use rostfrei_messaging_core::QueryAddress;
use thiserror::Error;

const DIRECT_QUERY_REGISTRATION: &str = "<direct query registration>";

/// Connects an owner-independent domain command to its executable Aggregate.
pub trait CommandDefinition<A>: domain::Command + Sized + Send + Sync + 'static
where
    A: Aggregate + CommandHandler<Self>,
{
    fn descriptor() -> CommandDescriptor {
        CommandDescriptor {
            command_name: Self::LOCAL_ID,
            schema_version: Self::SCHEMA_VERSION,
            aggregate_type: A::aggregate_type().into_owned(),
            rust_command_type: type_name::<Self>(),
            rust_aggregate_type: type_name::<A>(),
            modeled_command: Self::DESCRIPTOR,
        }
    }
}

impl<A, C> CommandDefinition<A> for C
where
    A: Aggregate + CommandHandler<C>,
    C: domain::Command + Sized + Send + Sync + 'static,
{
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CommandIdentity {
    pub aggregate_type: String,
    pub command_name: &'static str,
    pub schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub command_name: &'static str,
    pub schema_version: u32,
    pub aggregate_type: String,
    pub rust_command_type: &'static str,
    pub rust_aggregate_type: &'static str,
    pub modeled_command: domain::CommandDescriptor,
}

impl CommandDescriptor {
    pub fn identity(&self) -> CommandIdentity {
        CommandIdentity {
            aggregate_type: self.aggregate_type.clone(),
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
    #[error("command name must not be empty ({rust_command_type})")]
    EmptyCommandName { rust_command_type: &'static str },
    #[error("command `{command_name}` has schema version zero")]
    ZeroSchemaVersion { command_name: &'static str },
    #[error("command `{command_name}` version {schema_version} has an empty aggregate type")]
    EmptyAggregateType {
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "command `{command_name}` version {schema_version} is already registered for aggregate `{aggregate_type}`"
    )]
    DuplicateCommandIdentity {
        aggregate_type: String,
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
struct RegisteredQuery {
    module_name: &'static str,
    descriptor: QueryDescriptor,
}

#[derive(Debug, Default)]
pub struct DomainRegistry {
    commands: BTreeMap<&'static str, BTreeMap<u32, Vec<CommandDescriptor>>>,
    queries: BTreeMap<&'static str, BTreeMap<&'static str, BTreeMap<u32, RegisteredQuery>>>,
    aggregates: BTreeSet<String>,
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_command<A, C>(&mut self) -> Result<(), RegistrationError>
    where
        A: Aggregate + CommandHandler<C>,
        C: CommandDefinition<A>,
    {
        let command = <C as CommandDefinition<A>>::descriptor();
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

    pub fn commands(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands.values().flat_map(BTreeMap::values).flatten()
    }

    pub fn command(
        &self,
        aggregate_type: &str,
        command_name: &str,
        schema_version: u32,
    ) -> Option<&CommandDescriptor> {
        self.commands
            .get(command_name)
            .and_then(|versions| versions.get(&schema_version))
            .and_then(|registered| {
                registered
                    .iter()
                    .find(|command| command.aggregate_type == aggregate_type)
            })
    }

    pub fn commands_for_aggregate<'a>(
        &'a self,
        aggregate_type: &'a str,
    ) -> impl Iterator<Item = &'a CommandDescriptor> + 'a {
        self.commands()
            .filter(move |command| command.aggregate_type == aggregate_type)
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

    pub fn aggregates(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.aggregates.iter().map(String::as_str)
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
        if command.aggregate_type.trim().is_empty() {
            return Err(RegistrationError::EmptyAggregateType {
                command_name: command.command_name,
                schema_version: command.schema_version,
            });
        }
        if self
            .command(
                &command.aggregate_type,
                command.command_name,
                command.schema_version,
            )
            .is_some()
        {
            return Err(RegistrationError::DuplicateCommandIdentity {
                aggregate_type: command.aggregate_type.clone(),
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
        self.aggregates.insert(command.aggregate_type.clone());
        let registered = self
            .commands
            .entry(command.command_name)
            .or_default()
            .entry(command.schema_version)
            .or_default();
        registered.push(command);
        registered.sort_by(|left, right| left.aggregate_type.cmp(&right.aggregate_type));
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
