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

struct TypedCommandInputOptions<A, C, Provider>
where
    A: Aggregate + rostfrei_core::CommandHandler<C>,
    C: CommandDefinition<A>,
{
    provider: Provider,
    marker: std::marker::PhantomData<fn() -> (A, C)>,
}

#[async_trait]
impl<A, C, Provider> ErasedCommandInputOptions for TypedCommandInputOptions<A, C, Provider>
where
    A: Aggregate + rostfrei_core::CommandHandler<C>,
    C: CommandDefinition<A>,
    A::State: Send,
    A::Event: Event + Send,
    Provider: CommandInputOptions<A, C> + 'static,
{
    async fn fields(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
    ) -> Result<CommandInputDocument, RuntimeInputError> {
        let aggregate = Executor::new(history).rehydrate::<A>(&stream_id).await?;
        Ok(CommandInputDocument {
            fields: self.provider.fields(aggregate.state()),
        })
    }
}

struct TypedCommandSimulator<A, C>
where
    A: Aggregate + rostfrei_core::CommandHandler<C>,
    C: CommandDefinition<A>,
{
    descriptor: CommandDescriptor,
    marker: std::marker::PhantomData<fn() -> (A, C)>,
}

#[async_trait]
impl<A, C> ErasedCommandSimulator for TypedCommandSimulator<A, C>
where
    A: Aggregate + rostfrei_core::CommandHandler<C>,
    C: CommandDefinition<A> + JsonCommandPayload,
    A::State: Send,
    A::Event: Event + Send,
    <A as rostfrei_core::CommandHandler<C>>::Rejection: JsonErrorPayload,
{
    fn descriptor(&self) -> &CommandDescriptor {
        &self.descriptor
    }

    fn validate_payload(&self, payload: &Value) -> Result<(), String> {
        C::decode_json(payload).map(|_| ())
    }

    async fn simulate(
        &self,
        history: Arc<dyn EventHistory>,
        stream_id: StreamId,
        operation_id: OperationId,
        fingerprint: ContentFingerprint,
        payload: Value,
    ) -> Result<RuntimeDecision, RuntimeSimulationError> {
        let command = C::decode_json(&payload).map_err(RuntimeSimulationError::InvalidPayload)?;
        let metadata = ExecutionMetadata::new(stream_id, operation_id, fingerprint);
        let outcome = Executor::new(history)
            .simulate::<A, C>(metadata, &command)
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

    pub fn register_json<A, C>(&mut self) -> Result<(), RuntimeRegistrationError>
    where
        A: Aggregate + rostfrei_core::CommandHandler<C> + 'static,
        C: CommandDefinition<A> + JsonCommandPayload,
        A::State: Send,
        A::Event: Event + Send,
        <A as rostfrei_core::CommandHandler<C>>::Rejection: JsonErrorPayload,
    {
        let expected_descriptor = <C as CommandDefinition<A>>::descriptor();
        let descriptor = self
            .registry
            .command(
                &expected_descriptor.aggregate_type,
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
        let key = CommandKey::new(&descriptor.aggregate_type, C::LOCAL_ID, C::SCHEMA_VERSION);
        if self.simulators.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateBinding {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.simulators.insert(
            key,
            Arc::new(TypedCommandSimulator::<A, C> {
                descriptor,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub fn register_input_options<A, C, Provider>(
        &mut self,
        provider: Provider,
    ) -> Result<(), RuntimeRegistrationError>
    where
        A: Aggregate + rostfrei_core::CommandHandler<C> + 'static,
        C: CommandDefinition<A>,
        A::State: Send,
        A::Event: Event + Send,
        Provider: CommandInputOptions<A, C> + 'static,
    {
        let expected_descriptor = <C as CommandDefinition<A>>::descriptor();
        let descriptor = self
            .registry
            .command(
                &expected_descriptor.aggregate_type,
                C::LOCAL_ID,
                C::SCHEMA_VERSION,
            )
            .ok_or(RuntimeRegistrationError::MissingDescriptor {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            })?;
        if descriptor != &expected_descriptor {
            return Err(RuntimeRegistrationError::DescriptorMismatch {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        let key = CommandKey::new(&descriptor.aggregate_type, C::LOCAL_ID, C::SCHEMA_VERSION);
        if self.input_options.contains_key(&key) {
            return Err(RuntimeRegistrationError::DuplicateInputOptions {
                command: C::LOCAL_ID,
                schema_version: C::SCHEMA_VERSION,
            });
        }
        self.input_options.insert(
            key,
            Arc::new(TypedCommandInputOptions::<A, C, Provider> {
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
