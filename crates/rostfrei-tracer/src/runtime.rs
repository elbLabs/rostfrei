use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use domain::{JsonCommandPayload, JsonErrorPayload};
use rostfrei_core::{
    Aggregate, AggregateId, ContentFingerprint, Event, EventHistory, ExecutionMetadata, Executor,
    NewEvent, OperationId, SimulationDecision, SimulationError, StreamId,
};
use rostfrei_registry::{CommandDefinition, CommandDescriptor, DomainRegistry};
use serde_json::Value;
use thiserror::Error;

use crate::{
    input::{CommandInputDocument, CommandInputOptions},
    operation::PredictedDomainEvent,
};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RuntimeRegistrationError {
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
    #[error("registered command `{command}` version {schema_version} has no simulation binding")]
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
    #[error("a test repository requires a named test fixture")]
    TestRepositoryWithoutFixture,
    #[error("test definition `{id}` is invalid: {message}")]
    InvalidTestDefinition { id: String, message: String },
}

#[derive(Clone, Debug, Error)]
pub enum RuntimeSimulationError {
    #[error("invalid command payload: {0}")]
    InvalidPayload(String),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("rejection encoding failed: {0}")]
    RejectionEncoding(String),
    #[error("event payload is not valid JSON: {0}")]
    InvalidEventPayload(String),
    #[error("predicted stream version overflow")]
    StreamVersionOverflow,
}

#[derive(Clone, Debug, Error)]
pub enum RuntimeInputError {
    #[error(transparent)]
    Rehydration(#[from] SimulationError),
}

pub enum RuntimeDecision {
    Accepted {
        base_stream_version: u64,
        events: Vec<PredictedDomainEvent>,
    },
    Rejected {
        base_stream_version: u64,
        rejection: Value,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommandKey {
    pub aggregate_type: String,
    pub command: String,
    pub schema_version: u32,
}

impl CommandKey {
    pub fn new(
        aggregate_type: impl Into<String>,
        command: impl Into<String>,
        schema_version: u32,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            command: command.into(),
            schema_version,
        }
    }
}

#[async_trait]
pub trait ErasedCommandSimulator: Send + Sync {
    fn descriptor(&self) -> &CommandDescriptor;

    fn validate_payload(&self, payload: &Value) -> Result<(), String>;

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError>;
}

#[async_trait]
pub trait ErasedCommandInputOptions: Send + Sync {
    async fn fields(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
    ) -> Result<CommandInputDocument, RuntimeInputError>;
}

struct TypedCommandInputOptions<Command, Provider>
where
    Command: CommandDefinition,
{
    provider: Provider,
    marker: std::marker::PhantomData<fn() -> Command>,
}

#[async_trait]
impl<Command, Provider> ErasedCommandInputOptions for TypedCommandInputOptions<Command, Provider>
where
    Command: CommandDefinition,
    <Command::Aggregate as Aggregate>::State: Send,
    <Command::Aggregate as Aggregate>::Event: Event + Send,
    Provider: CommandInputOptions<Command> + 'static,
{
    async fn fields(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
    ) -> Result<CommandInputDocument, RuntimeInputError> {
        let aggregate = Executor::new(history)
            .rehydrate::<Command::Aggregate>(&stream_id)
            .await?;
        Ok(CommandInputDocument {
            fields: self.provider.fields(aggregate.state()),
        })
    }
}

struct TypedCommandSimulator<Command>
where
    Command: CommandDefinition,
{
    descriptor: CommandDescriptor,
    marker: std::marker::PhantomData<fn() -> Command>,
}

#[async_trait]
impl<Command> ErasedCommandSimulator for TypedCommandSimulator<Command>
where
    Command: CommandDefinition + JsonCommandPayload,
    Command::Aggregate: rostfrei_core::CommandHandler<Command>,
    <Command::Aggregate as Aggregate>::State: Send,
    <Command::Aggregate as rostfrei_core::Aggregate>::Event: Event + Send,
    <Command::Aggregate as rostfrei_core::CommandHandler<Command>>::Rejection: JsonErrorPayload,
{
    fn descriptor(&self) -> &CommandDescriptor {
        &self.descriptor
    }

    fn validate_payload(&self, payload: &Value) -> Result<(), String> {
        Command::decode_json(payload).map(|_| ())
    }

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError> {
        let command =
            Command::decode_json(&payload).map_err(RuntimeSimulationError::InvalidPayload)?;
        let metadata = ExecutionMetadata::new(stream_id, operation_id, fingerprint);
        let outcome = Executor::new(history)
            .simulate::<Command::Aggregate, Command>(metadata, &command)
            .await?;
        let (base_version, decision) = outcome.into_parts();
        match decision {
            SimulationDecision::Accepted(events) => Ok(RuntimeDecision::Accepted {
                base_stream_version: base_version.value(),
                events: events
                    .iter()
                    .enumerate()
                    .map(|(ordinal, event)| predicted_event(base_version.value(), ordinal, event))
                    .collect::<Result<_, _>>()?,
            }),
            SimulationDecision::Rejected(rejection) => Ok(RuntimeDecision::Rejected {
                base_stream_version: base_version.value(),
                rejection: rejection
                    .encode_json()
                    .map_err(RuntimeSimulationError::RejectionEncoding)?,
            }),
        }
    }
}

fn predicted_event(
    base_version: u64,
    ordinal: usize,
    event: &NewEvent,
) -> Result<PredictedDomainEvent, RuntimeSimulationError> {
    let ordinal =
        u32::try_from(ordinal).map_err(|_| RuntimeSimulationError::StreamVersionOverflow)?;
    let predicted_stream_version = base_version
        .checked_add(u64::from(ordinal) + 1)
        .ok_or(RuntimeSimulationError::StreamVersionOverflow)?;
    let payload = serde_json::from_slice(event.payload())
        .map_err(|error| RuntimeSimulationError::InvalidEventPayload(error.to_string()))?;
    Ok(PredictedDomainEvent {
        ordinal,
        predicted_stream_version,
        event_type: event.event_type().to_owned(),
        schema_version: event.schema_version(),
        payload: Some(payload),
    })
}

pub struct RuntimeBindings {
    pub registry: DomainRegistry,
    pub simulators: HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
    pub input_options: HashMap<CommandKey, Arc<dyn ErasedCommandInputOptions>>,
}

impl RuntimeBindings {
    pub fn new(registry: DomainRegistry) -> Self {
        Self {
            registry,
            simulators: HashMap::new(),
            input_options: HashMap::new(),
        }
    }

    pub fn register_json<Command>(&mut self) -> Result<(), RuntimeRegistrationError>
    where
        Command: CommandDefinition + JsonCommandPayload,
        Command::Aggregate: rostfrei_core::CommandHandler<Command>,
        <Command::Aggregate as Aggregate>::State: Send,
        <Command::Aggregate as Aggregate>::Event: Event + Send,
        <Command::Aggregate as rostfrei_core::CommandHandler<Command>>::Rejection: JsonErrorPayload,
    {
        let expected_descriptor = Command::descriptor();
        let descriptor = self
            .registry
            .command(
                &expected_descriptor.aggregate_type,
                Command::COMMAND_NAME,
                <Command as CommandDefinition>::SCHEMA_VERSION,
            )
            .cloned()
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: Command::COMMAND_NAME,
                schema_version: <Command as CommandDefinition>::SCHEMA_VERSION,
            })?;
        if descriptor != expected_descriptor {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: Command::COMMAND_NAME,
                schema_version: <Command as CommandDefinition>::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(
            &descriptor.aggregate_type,
            Command::COMMAND_NAME,
            <Command as CommandDefinition>::SCHEMA_VERSION,
        );
        if self.simulators.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateBinding {
                command: Command::COMMAND_NAME,
                schema_version: <Command as CommandDefinition>::SCHEMA_VERSION,
            });
        }
        self.simulators.insert(
            key,
            Arc::new(TypedCommandSimulator::<Command> {
                descriptor,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn register_input_options<Command, Provider>(
        &mut self,
        provider: Provider,
    ) -> Result<(), RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as Aggregate>::State: Send,
        <Command::Aggregate as Aggregate>::Event: Event + Send,
        Provider: CommandInputOptions<Command> + 'static,
    {
        let expected_descriptor = Command::descriptor();
        let descriptor = self
            .registry
            .command(
                &expected_descriptor.aggregate_type,
                Command::COMMAND_NAME,
                Command::SCHEMA_VERSION,
            )
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            })?;
        if descriptor != &expected_descriptor {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(
            &descriptor.aggregate_type,
            Command::COMMAND_NAME,
            Command::SCHEMA_VERSION,
        );
        if self.input_options.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateInputOptions {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            });
        }
        self.input_options.insert(
            key,
            Arc::new(TypedCommandInputOptions::<Command, Provider> {
                provider,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<(), RuntimeRegistrationError> {
        for descriptor in self.registry.commands() {
            let key = CommandKey::new(
                &descriptor.aggregate_type,
                descriptor.command_name,
                descriptor.schema_version,
            );
            if !self.simulators.contains_key(&key) {
                return Err(RuntimeRegistrationError::MissingBinding {
                    command: descriptor.command_name,
                    schema_version: descriptor.schema_version,
                });
            }
        }
        Ok(())
    }
}

pub fn stream_id(
    descriptor: &CommandDescriptor,
    aggregate_id: AggregateId,
) -> Result<StreamId, rostfrei_core::IdentityError> {
    Ok(StreamId::new(
        rostfrei_core::AggregateType::new(&descriptor.aggregate_type)?,
        aggregate_id,
    ))
}
