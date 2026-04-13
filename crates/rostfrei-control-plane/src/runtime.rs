use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rostfrei_core::{
    Aggregate, AggregateId, ContentFingerprint, Event, EventCodec, EventHistory, ExecutionMetadata,
    Executor, JsonEventCodec, NewEvent, OperationId, SimulationDecision, StreamId,
};
use rostfrei_registry::{CommandDefinition, CommandDescriptor, DomainRegistry};
use serde_json::Value;
use thiserror::Error;

use crate::operation::PredictedDomainEvent;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct CommandWireCodecError {
    message: String,
}

impl CommandWireCodecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub trait CommandWireCodec<Command>: Send + Sync
where
    Command: CommandDefinition,
{
    fn decode(&self, payload: &Value) -> Result<Command, CommandWireCodecError>;

    fn encode_rejection(
        &self,
        rejection: &<<Command as CommandDefinition>::Aggregate as rostfrei_core::CommandHandler<
            Command,
        >>::Rejection,
    ) -> Result<Value, CommandWireCodecError>;
}

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
}

#[derive(Clone, Debug, Error)]
pub(crate) enum RuntimeSimulationError {
    #[error("invalid command payload: {0}")]
    InvalidPayload(CommandWireCodecError),
    #[error("simulation failed: {0}")]
    Simulation(String),
    #[error("rejection encoding failed: {0}")]
    RejectionEncoding(CommandWireCodecError),
    #[error("predicted stream version overflow")]
    StreamVersionOverflow,
}

pub(crate) enum RuntimeDecision {
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
pub(crate) struct CommandKey {
    pub command: String,
    pub schema_version: u32,
}

impl CommandKey {
    pub fn new(command: impl Into<String>, schema_version: u32) -> Self {
        Self {
            command: command.into(),
            schema_version,
        }
    }
}

#[async_trait]
pub(crate) trait ErasedCommandSimulator: Send + Sync {
    fn descriptor(&self) -> &CommandDescriptor;

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError>;
}

struct TypedCommandSimulator<Command, Codec, Wire>
where
    Command: CommandDefinition,
{
    descriptor: CommandDescriptor,
    event_codec: Codec,
    wire_codec: Wire,
    marker: std::marker::PhantomData<fn() -> Command>,
}

#[async_trait]
impl<Command, Codec, Wire> ErasedCommandSimulator for TypedCommandSimulator<Command, Codec, Wire>
where
    Command: CommandDefinition,
    <Command::Aggregate as Aggregate>::State: Send,
    <Command::Aggregate as rostfrei_core::Aggregate>::Event: Send,
    Codec: EventCodec<Command::Aggregate> + Clone + Send + Sync + 'static,
    Wire: CommandWireCodec<Command> + 'static,
{
    fn descriptor(&self) -> &CommandDescriptor {
        &self.descriptor
    }

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError> {
        let command = self
            .wire_codec
            .decode(&payload)
            .map_err(RuntimeSimulationError::InvalidPayload)?;
        let metadata = ExecutionMetadata::new(stream_id, operation_id, fingerprint);
        let outcome = Executor::with_codec(history, self.event_codec.clone())
            .simulate::<Command::Aggregate, Command>(metadata, &command)
            .await
            .map_err(|error| RuntimeSimulationError::Simulation(error.to_string()))?;
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
                rejection: self
                    .wire_codec
                    .encode_rejection(&rejection)
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
    let payload = serde_json::from_slice(event.payload()).ok();
    let payload_base64 = payload.is_none().then(|| BASE64.encode(event.payload()));
    Ok(PredictedDomainEvent {
        ordinal,
        predicted_stream_version,
        event_type: event.event_type().to_owned(),
        schema_version: event.schema_version(),
        payload,
        payload_base64,
    })
}

pub(crate) struct RuntimeBindings {
    pub registry: DomainRegistry,
    pub simulators: HashMap<CommandKey, Arc<dyn ErasedCommandSimulator>>,
}

impl RuntimeBindings {
    pub fn new(registry: DomainRegistry) -> Self {
        Self {
            registry,
            simulators: HashMap::new(),
        }
    }

    pub fn register<Command, Wire>(
        &mut self,
        wire_codec: Wire,
    ) -> Result<(), RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as Aggregate>::State: Send,
        <Command::Aggregate as Aggregate>::Event: Event + Send,
        Wire: CommandWireCodec<Command> + 'static,
    {
        self.register_with_codec::<Command, JsonEventCodec, Wire>(JsonEventCodec, wire_codec)
    }

    pub fn register_with_codec<Command, Codec, Wire>(
        &mut self,
        event_codec: Codec,
        wire_codec: Wire,
    ) -> Result<(), RuntimeRegistrationError>
    where
        Command: CommandDefinition,
        <Command::Aggregate as Aggregate>::State: Send,
        <Command::Aggregate as Aggregate>::Event: Send,
        Codec: EventCodec<Command::Aggregate> + Clone + Send + Sync + 'static,
        Wire: CommandWireCodec<Command> + 'static,
    {
        let descriptor = self
            .registry
            .command(Command::COMMAND_NAME, Command::SCHEMA_VERSION)
            .cloned()
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            })?;
        if descriptor != Command::descriptor() {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(Command::COMMAND_NAME, Command::SCHEMA_VERSION);
        if self.simulators.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateBinding {
                command: Command::COMMAND_NAME,
                schema_version: Command::SCHEMA_VERSION,
            });
        }
        self.simulators.insert(
            key,
            Arc::new(TypedCommandSimulator::<Command, Codec, Wire> {
                descriptor,
                event_codec,
                wire_codec,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<(), RuntimeRegistrationError> {
        for descriptor in self.registry.commands() {
            let key = CommandKey::new(descriptor.command_name, descriptor.schema_version);
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

pub(crate) fn stream_id(
    descriptor: &CommandDescriptor,
    aggregate_id: AggregateId,
) -> Result<StreamId, rostfrei_core::IdentityError> {
    Ok(StreamId::new(
        rostfrei_core::AggregateType::new(&descriptor.aggregate_type)?,
        aggregate_id,
    ))
}
