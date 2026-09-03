use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};

use rostfrei_core::Aggregate;
use rostfrei_messaging_core::{CommandAddress, QueryAddress};
use thiserror::Error;

const DIRECT_COMMAND_REGISTRATION: &str = "<direct command registration>";
const DIRECT_QUERY_REGISTRATION: &str = "<direct query registration>";

pub trait CommandDefinition: Sized + Send + Sync + 'static {
    type Aggregate: Aggregate;

    const COMMAND_NAME: &'static str;
    const SCHEMA_VERSION: u32;

    fn descriptor() -> CommandDescriptor {
        CommandDescriptor {
            command_name: Self::COMMAND_NAME,
            schema_version: Self::SCHEMA_VERSION,
            aggregate_type: <Self::Aggregate as Aggregate>::aggregate_type().into_owned(),
            rust_command_type: type_name::<Self>(),
            rust_aggregate_type: type_name::<Self::Aggregate>(),
            modeled_command: None,
        }
    }
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
    pub modeled_command: Option<domain::CommandDescriptor>,
}

impl CommandDescriptor {
    pub fn identity(&self) -> CommandIdentity {
        CommandIdentity {
            aggregate_type: self.aggregate_type.clone(),
            command_name: self.command_name,
            schema_version: self.schema_version,
        }
    }

    pub const fn modeled_command(&self) -> Option<&domain::CommandDescriptor> {
        self.modeled_command.as_ref()
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

pub trait DomainModule: Send + Sync + 'static {
    const MODULE_NAME: &'static str;

    fn descriptor() -> ModuleDescriptor;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleDescriptor {
    pub module_name: &'static str,
    pub commands: Vec<CommandDescriptor>,
    pub queries: Vec<QueryDescriptor>,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RegistrationError {
    #[error("domain module name must not be empty")]
    EmptyModuleName,
    #[error("domain module `{module_name}` is already registered")]
    DuplicateModuleName { module_name: &'static str },
    #[error(
        "registration `{module_name}` contains a command with an empty name ({rust_command_type})"
    )]
    EmptyCommandName {
        module_name: &'static str,
        rust_command_type: &'static str,
    },
    #[error("command `{command_name}` in registration `{module_name}` has schema version zero")]
    ZeroSchemaVersion {
        module_name: &'static str,
        command_name: &'static str,
    },
    #[error(
        "command `{command_name}` version {schema_version} in registration `{module_name}` has an empty aggregate type"
    )]
    EmptyAggregateType {
        module_name: &'static str,
        command_name: &'static str,
        schema_version: u32,
    },
    #[error(
        "command `{command_name}` version {schema_version} in registration `{module_name}` has an invalid routing identity: {reason}"
    )]
    InvalidCommandIdentity {
        module_name: &'static str,
        command_name: &'static str,
        schema_version: u32,
        reason: String,
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
        "command `{command_name}` version {schema_version} from registration `{attempted_module_name}` is already registered by registration `{existing_module_name}`"
    )]
    DuplicateCommandIdentityAcrossModules {
        command_name: &'static str,
        schema_version: u32,
        existing_module_name: &'static str,
        attempted_module_name: &'static str,
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
        "query `{query_name}` version {schema_version} is duplicated inside domain module `{module_name}`"
    )]
    DuplicateQueryIdentityInModule {
        module_name: &'static str,
        query_name: &'static str,
        schema_version: u32,
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
struct RegisteredCommand {
    module_name: &'static str,
    descriptor: CommandDescriptor,
}

#[derive(Debug)]
struct RegisteredQuery {
    module_name: &'static str,
    descriptor: QueryDescriptor,
}

#[derive(Debug, Default)]
pub struct DomainRegistry {
    modules: BTreeMap<&'static str, ModuleDescriptor>,
    commands: BTreeMap<&'static str, BTreeMap<u32, Vec<RegisteredCommand>>>,
    queries: BTreeMap<&'static str, BTreeMap<&'static str, BTreeMap<u32, RegisteredQuery>>>,
    aggregates: BTreeSet<String>,
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_module<M: DomainModule>(&mut self) -> Result<(), RegistrationError> {
        let descriptor = M::descriptor();
        self.validate(&descriptor)?;

        for command in &descriptor.commands {
            self.insert_command(descriptor.module_name, command.clone());
        }
        for query in &descriptor.queries {
            self.insert_query(descriptor.module_name, query.clone());
        }
        self.modules.insert(descriptor.module_name, descriptor);
        Ok(())
    }

    pub fn register_command<C: CommandDefinition>(&mut self) -> Result<(), RegistrationError> {
        let command = C::descriptor();
        let registration = ModuleDescriptor {
            module_name: DIRECT_COMMAND_REGISTRATION,
            commands: vec![command.clone()],
            queries: Vec::new(),
        };
        self.validate_commands(&registration)?;
        self.insert_command(DIRECT_COMMAND_REGISTRATION, command);
        Ok(())
    }

    pub fn register_query<Q: QueryDefinition>(&mut self) -> Result<(), RegistrationError> {
        let query = Q::descriptor();
        let registration = ModuleDescriptor {
            module_name: DIRECT_QUERY_REGISTRATION,
            commands: Vec::new(),
            queries: vec![query.clone()],
        };
        self.validate_queries(&registration)?;
        self.insert_query(DIRECT_QUERY_REGISTRATION, query);
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
            .flatten()
            .map(|registered| &registered.descriptor)
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
                    .find(|command| command.descriptor.aggregate_type == aggregate_type)
            })
            .map(|registered| &registered.descriptor)
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

    fn validate(&self, module: &ModuleDescriptor) -> Result<(), RegistrationError> {
        if module.module_name.trim().is_empty() {
            return Err(RegistrationError::EmptyModuleName);
        }
        if self.modules.contains_key(module.module_name) {
            return Err(RegistrationError::DuplicateModuleName {
                module_name: module.module_name,
            });
        }

        self.validate_commands(module)?;
        self.validate_queries(module)
    }

    fn validate_commands(&self, module: &ModuleDescriptor) -> Result<(), RegistrationError> {
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
            let (bounded_context, aggregate) = match command.aggregate_type.split_once('/') {
                Some((bounded_context, aggregate)) if !aggregate.contains('/') => {
                    (bounded_context, aggregate)
                }
                Some(_) => {
                    return Err(RegistrationError::InvalidCommandIdentity {
                        module_name: module.module_name,
                        command_name: command.command_name,
                        schema_version: command.schema_version,
                        reason: "aggregate type must contain at most one context separator"
                            .to_owned(),
                    });
                }
                None => ("registry", command.aggregate_type.as_str()),
            };
            if let Err(error) =
                CommandAddress::new("rostfrei", bounded_context, command.command_name)
                    .and_then(|_| CommandAddress::new("rostfrei", "registry", aggregate))
            {
                return Err(RegistrationError::InvalidCommandIdentity {
                    module_name: module.module_name,
                    command_name: command.command_name,
                    schema_version: command.schema_version,
                    reason: error.to_string(),
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
            .and_then(|registered| {
                registered.iter().find(|registered| {
                    registered.descriptor.aggregate_type == command.aggregate_type
                })
            })
    }

    fn validate_queries(&self, module: &ModuleDescriptor) -> Result<(), RegistrationError> {
        let mut identities = BTreeSet::new();
        for query in &module.queries {
            if query.bounded_context.trim().is_empty() {
                return Err(RegistrationError::EmptyQueryBoundedContext {
                    module_name: module.module_name,
                    rust_request_type: query.rust_request_type,
                });
            }
            if query.query_name.trim().is_empty() {
                return Err(RegistrationError::EmptyQueryName {
                    module_name: module.module_name,
                    rust_request_type: query.rust_request_type,
                });
            }
            if query.schema_version == 0 {
                return Err(RegistrationError::ZeroQuerySchemaVersion {
                    module_name: module.module_name,
                    query_name: query.query_name,
                });
            }
            if let Err(error) =
                QueryAddress::new("rostfrei", query.bounded_context, query.query_name)
            {
                return Err(RegistrationError::InvalidQueryIdentity {
                    module_name: module.module_name,
                    query_name: query.query_name,
                    schema_version: query.schema_version,
                    reason: error.to_string(),
                });
            }

            if !identities.insert(query.identity()) {
                return Err(RegistrationError::DuplicateQueryIdentityInModule {
                    module_name: module.module_name,
                    query_name: query.query_name,
                    schema_version: query.schema_version,
                });
            }
            if let Some(existing) = self.registered_query(query) {
                return Err(RegistrationError::DuplicateQueryIdentityAcrossModules {
                    query_name: query.query_name,
                    schema_version: query.schema_version,
                    existing_module_name: existing.module_name,
                    attempted_module_name: module.module_name,
                });
            }
        }
        Ok(())
    }

    fn registered_query(&self, query: &QueryDescriptor) -> Option<&RegisteredQuery> {
        self.queries
            .get(query.bounded_context)
            .and_then(|queries| queries.get(query.query_name))
            .and_then(|versions| versions.get(&query.schema_version))
    }

    fn insert_command(&mut self, module_name: &'static str, command: CommandDescriptor) {
        self.aggregates.insert(command.aggregate_type.clone());
        let registered = self
            .commands
            .entry(command.command_name)
            .or_default()
            .entry(command.schema_version)
            .or_default();
        registered.push(RegisteredCommand {
            module_name,
            descriptor: command,
        });
        registered.sort_by(|left, right| {
            left.descriptor
                .aggregate_type
                .cmp(&right.descriptor.aggregate_type)
        });
    }

    fn insert_query(&mut self, module_name: &'static str, query: QueryDescriptor) {
        self.queries
            .entry(query.bounded_context)
            .or_default()
            .entry(query.query_name)
            .or_default()
            .insert(
                query.schema_version,
                RegisteredQuery {
                    module_name,
                    descriptor: query,
                },
            );
    }
}
