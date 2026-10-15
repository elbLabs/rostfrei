use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use rostfrei::{
    CommandHandler, CommandProcessor, CommandRejectionMapper, DomainRegistry, EventHistory,
    EventStore,
};
use rostfrei_core::{Aggregate, CommandExecution, CommandHandlingResult, Event};
use rostfrei_fixtures::MessageSeriesEngine;
use thiserror::Error;

use crate::{
    CommandInputOptions, RuntimeRegistrationError, TracerBuilder, runtime::RuntimeBindings,
};

#[derive(Debug, Error)]
pub enum ApplicationBindingError {
    #[error(transparent)]
    Registry(#[from] rostfrei::RegistrationError),
    #[error(transparent)]
    Preview(#[from] RuntimeRegistrationError),
    #[error(transparent)]
    Fixture(#[from] rostfrei_fixtures::FixtureCodecRegistrationError),
    #[error(transparent)]
    Command(#[from] rostfrei::CommandBindingRegistrationError),
}

type ProcessorBinding =
    Box<dyn Fn(&mut CommandProcessor) -> Result<(), ApplicationBindingError> + Send + Sync>;
type IntegrationBinding = Box<
    dyn Fn(
            rostfrei::IntegrationEventBus,
            &mut rostfrei::DomainEventDispatcher,
        ) -> Result<(), rostfrei::DomainEventRegistrationError>
        + Send
        + Sync,
>;
type IntegrationDecoder =
    fn(&rostfrei::EncodedIntegrationMessage) -> Result<crate::IntegrationEventObservation, String>;
type IntegrationHandler = Arc<
    dyn rostfrei_messaging_core::MessageHandler<rostfrei_messaging_core::IntegrationEventAddress>,
>;
type IntegrationHandlerFactory = Box<
    dyn Fn(
            rostfrei::CommandBus,
            rostfrei_messaging_core::DurableName,
            rostfrei_messaging_core::RetryDelay,
        ) -> IntegrationHandler
        + Send
        + Sync,
>;

#[cfg_attr(
    not(feature = "nats-host"),
    allow(
        dead_code,
        reason = "retained typed registrations are consumed by the optional host"
    )
)]
pub struct IntegrationHandlerBinding {
    pub producer: Option<&'static str>,
    pub name: &'static str,
    pub handler: IntegrationHandlerFactory,
}

#[cfg(feature = "nats-host")]
impl IntegrationHandlerBinding {
    pub fn address(
        &self,
        consumer: &rostfrei::BoundedContext,
    ) -> Result<
        rostfrei_messaging_core::IntegrationEventAddress,
        rostfrei_messaging_core::ContractError,
    > {
        let producer = self.producer.map_or_else(
            || Ok(consumer.clone()),
            |name| {
                consumer
                    .application()
                    .bounded_context_in_scope(consumer.traffic_scope(), name)
            },
        )?;
        producer.integration_event_address(self.name)
    }
}

/// Executable application composition shared by Preview and transported execution.
/// Handler instances (and their dependencies) are retained, not reconstructed.
pub struct ApplicationBindings {
    pub(crate) preview: RuntimeBindings,
    pub(crate) fixtures: MessageSeriesEngine,
    processors: Vec<ProcessorBinding>,
    pub(crate) commands: Vec<rostfrei::CommandRegistrationDescriptor>,
    domain_model: Option<serde_json::Value>,
    pub(crate) integration_mappings: Vec<IntegrationBinding>,
    pub(crate) integration_decoders:
        HashMap<(Option<&'static str>, &'static str, u32), IntegrationDecoder>,
    pub(crate) integration_handlers: Vec<IntegrationHandlerBinding>,
}

impl Default for ApplicationBindings {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationBindings {
    pub fn new() -> Self {
        Self {
            preview: RuntimeBindings::new(DomainRegistry::new()),
            fixtures: MessageSeriesEngine::new(),
            processors: Vec::new(),
            commands: Vec::new(),
            domain_model: None,
            integration_mappings: Vec::new(),
            integration_decoders: HashMap::new(),
            integration_handlers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_domain_model(mut self, model: serde_json::Value) -> Self {
        self.domain_model = Some(model);
        self
    }

    pub fn register_aggregate<A>(&mut self) -> Result<&mut Self, ApplicationBindingError>
    where
        A: Aggregate + 'static,
        A::Event: Event,
    {
        self.preview.registry.register_aggregate::<A>()?;
        self.fixtures.register_json::<A>()?;
        Ok(self)
    }

    pub fn register_command<C, H>(
        &mut self,
        handler: H,
        mapper: impl CommandRejectionMapper<H::Rejection> + 'static,
    ) -> Result<&mut Self, ApplicationBindingError>
    where
        C: rostfrei::JsonCommandPayload + Send + Sync + 'static,
        H: CommandHandler<C> + 'static,
        H::Rejection: rostfrei::JsonErrorPayload + 'static,
    {
        let handler = SharedHandler(Arc::new(handler));
        self.preview.registry.register_command::<C, H>()?;
        self.preview
            .register_shared_json::<C, _>(handler.0.clone())?;
        self.commands
            .push(<C as rostfrei::CommandDefinition<H>>::descriptor());
        let mapper = Arc::new(mapper);
        self.processors.push(Box::new(move |processor| {
            let mapper = mapper.clone();
            processor.register::<C, _>(handler.clone(), move |rejection: &H::Rejection| {
                mapper.map(rejection)
            })?;
            Ok(())
        }));
        Ok(self)
    }

    pub fn register_input_options<C, P>(
        &mut self,
        provider: P,
    ) -> Result<&mut Self, ApplicationBindingError>
    where
        C: rostfrei::Command + Sync,
        P: CommandInputOptions<C> + 'static,
    {
        self.preview.register_input_options::<C, P>(provider)?;
        Ok(self)
    }

    pub fn command_processor(
        &self,
        context: rostfrei::BoundedContextName,
        store: Arc<dyn EventStore>,
    ) -> Result<CommandProcessor, ApplicationBindingError> {
        let mut processor = CommandProcessor::new(context, store);
        for register in &self.processors {
            register(&mut processor)?;
        }
        Ok(processor)
    }

    pub fn tracer_builder(&self, history: Arc<dyn EventHistory>) -> TracerBuilder {
        let mut builder = TracerBuilder::new(history, self.preview.registry.clone())
            .with_bindings(self.preview.clone());
        if let Some(model) = &self.domain_model {
            builder = builder.with_domain_model(model.clone());
        }
        builder
    }

    pub const fn registry(&self) -> &DomainRegistry {
        &self.preview.registry
    }

    pub fn register_integration_event<A, D, M>(&mut self, mapper: M) -> &mut Self
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Event + rostfrei::EventVariant<D> + Send + Sync + 'static,
        D: rostfrei::DomainEvent + Send + Sync,
        M: rostfrei::IntegrationEventMapper<D> + 'static,
    {
        use rostfrei::IntegrationEventDispatcherExt as _;
        self.register_integration_codec::<M::Output>();
        let mapper = Arc::new(mapper);
        self.integration_mappings
            .push(Box::new(move |bus, dispatcher| {
                dispatcher.register_integration_event::<A, D, _>(bus, SharedMapper(mapper.clone()))
            }));
        self
    }

    pub fn register_integration_handler<E, H>(&mut self, handler: H) -> &mut Self
    where
        E: rostfrei::IntegrationEvent,
        H: rostfrei_messaging_core::MessageHandler<
                rostfrei_messaging_core::IntegrationEventAddress,
            > + 'static,
    {
        self.register_integration_codec::<E>();
        let handler = Arc::new(handler);
        self.integration_handlers.push(IntegrationHandlerBinding {
            producer: E::BOUNDED_CONTEXT,
            name: E::EVENT_NAME,
            handler: Box::new(move |_, _, _| handler.clone()),
        });
        self
    }

    /// Map a typed integration event to a command using host-owned bus, durable
    /// identity and retry policy. The event contract selects the producer context;
    /// an undeclared producer falls back to the consuming runtime's context.
    pub fn register_integration_command<E, M>(&mut self, mapper: M) -> &mut Self
    where
        E: rostfrei::IntegrationEvent,
        M: rostfrei::IntegrationCommandMapper<E> + 'static,
        M::Error: Send,
    {
        self.register_integration_codec::<E>();
        let mapper = Arc::new(mapper);
        self.integration_handlers.push(IntegrationHandlerBinding {
            producer: E::BOUNDED_CONTEXT,
            name: E::EVENT_NAME,
            handler: Box::new(move |bus, durable, retry| {
                Arc::new(rostfrei::IntegrationEventCommandHandler::<E, _>::new(
                    bus,
                    durable,
                    retry,
                    SharedCommandMapper(mapper.clone()),
                ))
            }),
        });
        self
    }

    fn register_integration_codec<E: rostfrei::IntegrationEvent>(&mut self) {
        self.integration_decoders.insert(
            (E::BOUNDED_CONTEXT, E::EVENT_NAME, E::SCHEMA_VERSION),
            decode_integration::<E>,
        );
    }
}

fn decode_integration<E: rostfrei::IntegrationEvent>(
    message: &rostfrei::EncodedIntegrationMessage,
) -> Result<crate::IntegrationEventObservation, String> {
    let envelope = message.decode::<E>().map_err(|error| error.to_string())?;
    Ok(crate::IntegrationEventObservation::new(
        envelope.message_id().as_str(),
        E::EVENT_NAME,
        envelope.schema_version().get(),
        message.address().as_str(),
    )
    .with_causation_id(
        envelope
            .causation_id()
            .ok_or("integration event has no causation identity")?
            .as_str(),
    )
    .with_payload(serde_json::to_value(envelope.payload()).map_err(|error| error.to_string())?))
}

struct SharedMapper<M>(Arc<M>);
struct SharedCommandMapper<M>(Arc<M>);
impl<E, M: rostfrei::IntegrationCommandMapper<E>> rostfrei::IntegrationCommandMapper<E>
    for SharedCommandMapper<M>
{
    type Command = M::Command;
    type Error = M::Error;
    fn map(&self, event: &E) -> Result<Self::Command, Self::Error> {
        self.0.map(event)
    }
}

impl<D, M: rostfrei::IntegrationEventMapper<D>> rostfrei::IntegrationEventMapper<D>
    for SharedMapper<M>
{
    type Output = M::Output;
    fn map(&self, event: &rostfrei::CommittedDomainEvent<'_, D>) -> Self::Output {
        self.0.map(event)
    }
}

struct SharedHandler<H>(Arc<H>);
impl<H> Clone for SharedHandler<H> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
#[async_trait]
impl<C: Sync, H: CommandHandler<C>> CommandHandler<C> for SharedHandler<H> {
    type Rejection = H::Rejection;
    async fn handle(
        &self,
        command: &C,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        self.0.handle(command, execution).await
    }
}
