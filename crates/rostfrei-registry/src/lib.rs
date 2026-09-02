use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};

use rostfrei_core::{Aggregate, CommandHandler};
use thiserror::Error;

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
}

#[derive(Debug, Default)]
pub struct DomainRegistry {
    commands: BTreeMap<&'static str, BTreeMap<u32, Vec<CommandDescriptor>>>,
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
}
