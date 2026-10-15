use std::{any::type_name, collections::HashMap, sync::Arc};

use domain::{JsonCommandPayload, JsonErrorPayload};
use rostfrei_core::CommandHandler;
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde_json::Value;
use thiserror::Error;

use crate::input::{CommandInputDocument, CommandInputOptions};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RuntimeRegistrationError {
    #[error("quarantine reader does not match its configured {scope} traffic scope")]
    InvalidQuarantineScope { scope: &'static str },
    #[error("Test and production quarantine readers must belong to the same application")]
    QuarantineApplicationMismatch,
    #[error("command `{command}` version {schema_version} is not in the domain registry")]
    MissingDescriptor {
        command: &'static str,
        schema_version: u32,
    },
    #[error("command `{command}` version {schema_version} is already bound")]
    DuplicateBinding {
        command: &'static str,
        schema_version: u32,
    },
    #[error("command `{command}` version {schema_version} does not match its registry descriptor")]
    DescriptorMismatch {
        command: &'static str,
        schema_version: u32,
    },
    #[error("registered command `{command}` version {schema_version} has no payload binding")]
    MissingBinding {
        command: &'static str,
        schema_version: u32,
    },
    #[error("command `{command}` version {schema_version} already has an input-options binding")]
    DuplicateInputOptions {
        command: &'static str,
        schema_version: u32,
    },
    #[error("a test scenario reset requires a configured test event store")]
    ResetWithoutTestStore,
    #[error("a test scenario reset requires a configured test command transport")]
    ResetWithoutTestTransport,
    #[error("a test scenario reset requires an explicit default test fixture")]
    ResetWithoutDefaultTestFixture,
    #[error("test fixtures require a configured test scenario reset")]
    TestFixtureWithoutReset,
    #[error("test fixture `{fixture_id}` is registered more than once")]
    DuplicateTestFixture { fixture_id: String },
    #[error("default test fixture `{second}` conflicts with existing default `{first}`")]
    MultipleDefaultTestFixtures { first: String, second: String },
    #[error("a test repository requires a named test fixture")]
    TestRepositoryWithoutFixture,
    #[error("test definition `{id}` is invalid: {message}")]
    InvalidTestDefinition { id: String, message: String },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandKey {
    pub context: String,
    pub command: String,
    pub schema_version: u32,
}

impl CommandKey {
    pub fn new(
        context: impl Into<String>,
        command: impl Into<String>,
        schema_version: u32,
    ) -> Self {
        Self {
            context: context.into(),
            command: command.into(),
            schema_version,
        }
    }
}

pub trait ErasedCommandPayload: Send + Sync {
    fn validate_payload(&self, payload: &Value) -> Result<(), String>;
}

pub trait ErasedCommandInputOptions: Send + Sync {
    fn fields(&self) -> CommandInputDocument;
}

struct TypedCommandInputOptions<C, Provider> {
    provider: Provider,
    marker: std::marker::PhantomData<fn() -> C>,
}

impl<C, Provider> ErasedCommandInputOptions for TypedCommandInputOptions<C, Provider>
where
    C: Sync,
    Provider: CommandInputOptions<C> + 'static,
{
    fn fields(&self) -> CommandInputDocument {
        CommandInputDocument {
            fields: self.provider.fields(),
        }
    }
}

struct TypedCommandPayload<C, Handler>
where
    Handler: CommandHandler<C>,
    C: CommandDefinition<Handler> + Sync,
{
    _handler: Arc<Handler>,
    marker: std::marker::PhantomData<fn() -> C>,
}

impl<C, Handler> ErasedCommandPayload for TypedCommandPayload<C, Handler>
where
    Handler: CommandHandler<C> + 'static,
    C: CommandDefinition<Handler> + JsonCommandPayload + Sync,
    Handler::Rejection: JsonErrorPayload,
{
    fn validate_payload(&self, payload: &Value) -> Result<(), String> {
        C::decode_json(payload).map(|_| ())
    }
}

#[derive(Clone)]
pub struct RuntimeBindings {
    pub registry: DomainRegistry,
    pub payloads: HashMap<CommandKey, Arc<dyn ErasedCommandPayload>>,
    pub input_options: HashMap<CommandKey, Arc<dyn ErasedCommandInputOptions>>,
}

impl RuntimeBindings {
    pub fn new(registry: DomainRegistry) -> Self {
        Self {
            registry,
            payloads: HashMap::new(),
            input_options: HashMap::new(),
        }
    }

    pub fn register_json<C, Handler>(
        &mut self,
        handler: Handler,
    ) -> Result<(), RuntimeRegistrationError>
    where
        Handler: CommandHandler<C> + 'static,
        C: CommandDefinition<Handler> + JsonCommandPayload + Sync,
        Handler::Rejection: JsonErrorPayload,
    {
        self.register_shared_json::<C, Handler>(Arc::new(handler))
    }

    pub fn register_shared_json<C, Handler>(
        &mut self,
        handler: Arc<Handler>,
    ) -> Result<(), RuntimeRegistrationError>
    where
        Handler: CommandHandler<C> + 'static,
        C: CommandDefinition<Handler> + JsonCommandPayload + Sync,
        Handler::Rejection: JsonErrorPayload,
    {
        let expected_descriptor = <C as CommandDefinition<Handler>>::descriptor();
        let descriptor = self
            .registry
            .command(
                expected_descriptor.bounded_context,
                C::LOCAL_ID,
                C::SCHEMA_VERSION,
            )
            .cloned()
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            })?;
        if descriptor != expected_descriptor {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(descriptor.bounded_context, C::LOCAL_ID, C::SCHEMA_VERSION);
        if self.payloads.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateBinding {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.payloads.insert(
            key,
            Arc::new(TypedCommandPayload::<C, Handler> {
                _handler: handler,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn register_input_options<C, Provider>(
        &mut self,
        provider: Provider,
    ) -> Result<(), RuntimeRegistrationError>
    where
        C: domain::Command + Sync,
        Provider: CommandInputOptions<C> + 'static,
    {
        let context = <C::Context as domain::BoundedContextType>::DESCRIPTOR.id.0;
        let descriptor = self
            .registry
            .command(context, C::LOCAL_ID, C::SCHEMA_VERSION)
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            })?;
        if descriptor.rust_command_type != type_name::<C>()
            || descriptor.modeled_command() != &C::DESCRIPTOR
        {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(descriptor.bounded_context, C::LOCAL_ID, C::SCHEMA_VERSION);
        if self.input_options.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateInputOptions {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.input_options.insert(
            key,
            Arc::new(TypedCommandInputOptions::<C, Provider> {
                provider,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<(), RuntimeRegistrationError> {
        for descriptor in self.registry.commands() {
            let key = CommandKey::new(
                descriptor.bounded_context,
                descriptor.command_name,
                descriptor.schema_version,
            );
            if !self.payloads.contains_key(&key) {
                return Err(RuntimeRegistrationError::MissingBinding {
                    command: descriptor.command_name,
                    schema_version: descriptor.schema_version,
                });
            }
        }
        Ok(())
    }
}
