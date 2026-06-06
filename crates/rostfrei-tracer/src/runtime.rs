use std::{any::type_name, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use domain::{BoundedContextType, JsonCommandPayload, JsonErrorPayload};
use rostfrei_core::{
    CommandDecision, CommandExecutionMetadata, CommandExecutor, CommandHandler, ContentFingerprint,
    EventHistory, NewEvent, OperationId, SimulationError, StreamId,
};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde_json::Value;
use thiserror::Error;

use crate::{
    input::{CommandInputDocument, CommandInputOptions},
    operation::{PredictedDomainEvent, TouchedStreamParticipant},
};

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

#[derive(Clone, Debug, Error)]
pub enum RuntimeSimulationError {
    #[error("invalid command payload: {0}")]
    InvalidPayload(String),
    #[error("invalid command bounded context: {0}")]
    InvalidBoundedContext(String),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("rejection encoding failed: {0}")]
    RejectionEncoding(String),
    #[error("event payload is not valid JSON: {0}")]
    InvalidEventPayload(String),
    #[error("predicted stream version overflow")]
    StreamVersionOverflow,
}

pub enum RuntimeDecision {
    Accepted {
        participants: Vec<TouchedStreamParticipant>,
        events: Vec<PredictedDomainEvent>,
    },
    Rejected {
        participants: Vec<TouchedStreamParticipant>,
        rejection: Value,
    },
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

#[async_trait]
pub trait ErasedCommandSimulator: Send + Sync {
    fn validate_payload(&self, payload: &Value) -> Result<(), String>;

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError>;
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

struct TypedCommandSimulator<C, Handler>
where
    Handler: CommandHandler<C>,
    C: CommandDefinition<Handler> + Sync,
{
    handler: Handler,
    marker: std::marker::PhantomData<fn() -> C>,
}

#[async_trait]
impl<C, Handler> ErasedCommandSimulator for TypedCommandSimulator<C, Handler>
where
    Handler: CommandHandler<C> + 'static,
    C: CommandDefinition<Handler> + JsonCommandPayload + Sync,
    Handler::Rejection: JsonErrorPayload,
{
    fn validate_payload(&self, payload: &Value) -> Result<(), String> {
        C::decode_json(payload).map(|_| ())
    }

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError> {
        let command = C::decode_json(&payload).map_err(RuntimeSimulationError::InvalidPayload)?;
        let context = rostfrei_messaging_core::BoundedContextName::new(
            <C::Context as BoundedContextType>::DESCRIPTOR.id.0,
        )
        .map_err(|error| RuntimeSimulationError::InvalidBoundedContext(error.to_string()))?;
        let metadata =
            CommandExecutionMetadata::new(operation_id, fingerprint).with_bounded_context(context);
        let (decision, simulated) = CommandExecutor::new(history)
            .simulate(&self.handler, metadata, &command)
            .await?
            .into_parts();
        let participants = simulated
            .iter()
            .map(|participant| TouchedStreamParticipant {
                aggregate_type: participant.stream_id().aggregate_type().as_str().to_owned(),
                aggregate_id: participant.stream_id().aggregate_id().as_str().to_owned(),
                base_stream_version: participant.base_version().value(),
                read_guard: participant.is_read_guard(),
            })
            .collect();
        match decision {
            CommandDecision::Accepted => Ok(RuntimeDecision::Accepted {
                participants,
                events: simulated
                    .iter()
                    .flat_map(|participant| {
                        participant
                            .events()
                            .iter()
                            .enumerate()
                            .map(|(ordinal, event)| {
                                predicted_event(
                                    participant.stream_id(),
                                    participant.base_version().value(),
                                    ordinal,
                                    event,
                                )
                            })
                    })
                    .collect::<Result<_, _>>()?,
            }),
            CommandDecision::Rejected(rejection) => Ok(RuntimeDecision::Rejected {
                participants,
                rejection: rejection
                    .encode_json()
                    .map_err(RuntimeSimulationError::RejectionEncoding)?,
            }),
        }
    }
}

fn predicted_event(
    stream_id: &StreamId,
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
        aggregate_type: stream_id.aggregate_type().as_str().to_owned(),
        aggregate_id: stream_id.aggregate_id().as_str().to_owned(),
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

    pub fn register_json<C, Handler>(
        &mut self,
        handler: Handler,
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
        if self.simulators.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateBinding {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.simulators.insert(
            key,
            Arc::new(TypedCommandSimulator::<C, Handler> {
                handler,
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
