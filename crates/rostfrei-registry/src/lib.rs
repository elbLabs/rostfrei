use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};

use rostfrei_core::{Aggregate, CommandHandler};
use thiserror::Error;

pub trait CommandDefinition: Sized + Send + Sync + 'static {
    type Aggregate: Aggregate + CommandHandler<Self>;

    const COMMAND_NAME: &'static str;
    const SCHEMA_VERSION: u32;

    fn descriptor() -> CommandDescriptor {
        CommandDescriptor {
            command_name: Self::COMMAND_NAME,
            schema_version: Self::SCHEMA_VERSION,
            aggregate_type: <Self::Aggregate as Aggregate>::AGGREGATE_TYPE,
            rust_command_type: type_name::<Self>(),
            rust_aggregate_type: type_name::<Self::Aggregate>(),
            domain_command: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CommandIdentity {
    pub command_name: &'static str,
    pub schema_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub command_name: &'static str,
    pub schema_version: u32,
    pub aggregate_type: &'static str,
    pub rust_command_type: &'static str,
    pub rust_aggregate_type: &'static str,
    pub domain_command: Option<domain::DomainCommandDescriptor>,
}

impl CommandDescriptor {
    pub const fn identity(&self) -> CommandIdentity {
        CommandIdentity {
            command_name: self.command_name,
            schema_version: self.schema_version,
        }
    }

    pub const fn domain_command(&self) -> Option<&domain::DomainCommandDescriptor> {
        self.domain_command.as_ref()
    }
}

pub trait DomainModule: Send + Sync + 'static {
    const MODULE_NAME: &'static str;

    fn descriptor() -> ModuleDescriptor;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleDescriptor {
    pub module_name: &'static str,
    pub commands: Vec<CommandDescriptor>,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RegistrationError {
    #[error("domain module name must not be empty")]
    EmptyModuleName,
    #[error("domain module `{module_name}` is already registered")]
    DuplicateModuleName { module_name: &'static str },
    #[error(
        "domain module `{module_name}` contains a command with an empty name ({rust_command_type})"
    )]
    EmptyCommandName {
        module_name: &'static str,
        rust_command_type: &'static str,
    },
    #[error("command `{command_name}` in domain module `{module_name}` has schema version zero")]
    ZeroSchemaVersion {
        module_name: &'static str,
        command_name: &'static str,
    },
    #[error(
        "command `{command_name}` version {schema_version} in domain module `{module_name}` has an empty aggregate type"
    )]
    EmptyAggregateType {
        module_name: &'static str,
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "command `{command_name}` version {schema_version} is duplicated inside domain module `{module_name}`"
    )]
    DuplicateCommandIdentityInModule {
        module_name: &'static str,
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "command `{command_name}` version {schema_version} from domain module `{attempted_module_name}` is already registered by domain module `{existing_module_name}`"
    )]
    DuplicateCommandIdentityAcrossModules {
        command_name: &'static str,
        schema_version: u32,
        existing_module_name: &'static str,
        attempted_module_name: &'static str,
    },
}

#[derive(Debug)]
struct RegisteredCommand {
    module_name: &'static str,
    descriptor: CommandDescriptor,
}

#[derive(Debug, Default)]
pub struct DomainRegistry {
    modules: BTreeMap<&'static str, ModuleDescriptor>,
    commands: BTreeMap<&'static str, BTreeMap<u32, RegisteredCommand>>,
    aggregates: BTreeSet<&'static str>,
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_module<M: DomainModule>(&mut self) -> Result<(), RegistrationError> {
        let descriptor = M::descriptor();
        self.validate(&descriptor)?;

        for command in &descriptor.commands {
            self.aggregates.insert(command.aggregate_type);
            self.commands
                .entry(command.command_name)
                .or_default()
                .insert(
                    command.schema_version,
                    RegisteredCommand {
                        module_name: descriptor.module_name,
                        descriptor: command.clone(),
                    },
                );
        }
        self.modules.insert(descriptor.module_name, descriptor);
        Ok(())
    }

    pub fn modules(&self) -> impl ExactSizeIterator<Item = &ModuleDescriptor> {
        self.modules.values()
    }

    pub fn module(&self, module_name: &str) -> Option<&ModuleDescriptor> {
        self.modules.get(module_name)
    }

    pub fn commands(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands
            .values()
            .flat_map(BTreeMap::values)
            .map(|registered| &registered.descriptor)
    }

    pub fn command(&self, command_name: &str, schema_version: u32) -> Option<&CommandDescriptor> {
        self.commands
            .get(command_name)
            .and_then(|versions| versions.get(&schema_version))
            .map(|registered| &registered.descriptor)
    }

    pub fn commands_for_aggregate<'a>(
        &'a self,
        aggregate_type: &'a str,
    ) -> impl Iterator<Item = &'a CommandDescriptor> + 'a {
        self.commands()
            .filter(move |command| command.aggregate_type == aggregate_type)
    }

    pub fn aggregates(&self) -> impl ExactSizeIterator<Item = &'static str> + '_ {
        self.aggregates.iter().copied()
    }

    fn validate(&self, module: &ModuleDescriptor) -> Result<(), RegistrationError> {
        if module.module_name.trim().is_empty() {
            return Err(RegistrationError::EmptyModuleName);
        }
        if self.modules.contains_key(module.module_name) {
            return Err(RegistrationError::DuplicateModuleName {
                module_name: module.module_name,
            });
        }

        let mut identities = BTreeSet::new();
        for command in &module.commands {
            if command.command_name.trim().is_empty() {
                return Err(RegistrationError::EmptyCommandName {
                    module_name: module.module_name,
                    rust_command_type: command.rust_command_type,
                });
            }
            if command.schema_version == 0 {
                return Err(RegistrationError::ZeroSchemaVersion {
                    module_name: module.module_name,
                    command_name: command.command_name,
                });
            }
            if command.aggregate_type.trim().is_empty() {
                return Err(RegistrationError::EmptyAggregateType {
                    module_name: module.module_name,
                    command_name: command.command_name,
                    schema_version: command.schema_version,
                });
            }

            let identity = command.identity();
            if !identities.insert(identity) {
                return Err(RegistrationError::DuplicateCommandIdentityInModule {
                    module_name: module.module_name,
                    command_name: command.command_name,
                    schema_version: command.schema_version,
                });
            }
            if let Some(existing) = self.registered_command(command) {
                return Err(RegistrationError::DuplicateCommandIdentityAcrossModules {
                    command_name: command.command_name,
                    schema_version: command.schema_version,
                    existing_module_name: existing.module_name,
                    attempted_module_name: module.module_name,
                });
            }
        }
        Ok(())
    }

    fn registered_command(&self, command: &CommandDescriptor) -> Option<&RegisteredCommand> {
        self.commands
            .get(command.command_name)
            .and_then(|versions| versions.get(&command.schema_version))
    }
}
