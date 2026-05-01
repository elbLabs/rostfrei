use std::any::Any;

use rostfrei_core::{Aggregate, AggregateId, CommandHandler, OperationId};
use rostfrei_messaging_core::{CausationId, DurableName, IntegrationEventEnvelope};
use rostfrei_registry::CommandDefinition;
use thiserror::Error;

use crate::{
    CommandBus, CommandBusError, CommandBusReceipt, DynamicCommandRequest, JsonCommandPayload,
    RoutedAggregateCommand, RoutedAggregateCommandError, command_bus::framed_fingerprint,
};

/// Maps one incoming integration event to at most one aggregate command.
pub trait IntegrationEventHandler<E>: Send + Sync {
    type Error;

    fn handle(&self, event: &E, commands: &mut CommandContext) -> Result<(), Self::Error>;
}

/// Collects a typed command while an integration event is being mapped.
///
/// Calling [`Self::issue`] performs no I/O. The processor dispatches only after
/// the handler returns successfully and exactly one command was issued.
#[derive(Default)]
pub struct CommandContext {
    issued_count: usize,
    command: Option<Result<IssuedCommand, CommandContextError>>,
}

impl CommandContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue<A, C>(&mut self, aggregate_id: AggregateId, command: C)
    where
        A: Aggregate + CommandHandler<C>,
        C: CommandDefinition<A> + JsonCommandPayload + Send + Sync,
    {
        self.issued_count = self.issued_count.saturating_add(1);
        if self.issued_count != 1 {
            return;
        }

        let routed = command
            .encode_json()
            .map_err(CommandContextError::Encoding)
            .and_then(|payload| {
                RoutedAggregateCommand::new(
                    A::aggregate_type().into_owned(),
                    aggregate_id.as_str(),
                    C::LOCAL_ID,
                    C::SCHEMA_VERSION,
                    payload,
                )
                .map_err(CommandContextError::InvalidCommand)
            });
        self.command = Some(routed.map(|routed| IssuedCommand {
            aggregate_id,
            command: Box::new(command),
            routed,
        }));
    }

    pub const fn issued_count(&self) -> usize {
        self.issued_count
    }

    pub const fn is_empty(&self) -> bool {
        self.issued_count == 0
    }

    pub fn issued_command(&self) -> Option<&RoutedAggregateCommand> {
        if self.issued_count != 1 {
            return None;
        }
        self.command
            .as_ref()?
            .as_ref()
            .ok()
            .map(|command| &command.routed)
    }

    /// Returns a typed view of the issued command for focused mapper tests.
    pub fn issued<C>(&self) -> Option<(&AggregateId, &C)>
    where
        C: 'static,
    {
        if self.issued_count != 1 {
            return None;
        }
        let command = self.command.as_ref()?.as_ref().ok()?;
        Some((&command.aggregate_id, command.command.downcast_ref::<C>()?))
    }

    fn into_command(self) -> Result<Option<RoutedAggregateCommand>, CommandContextError> {
        match self.issued_count {
            0 => Ok(None),
            1 => self
                .command
                .ok_or_else(|| {
                    CommandContextError::Encoding(
                        "issued command intent was not recorded".to_owned(),
                    )
                })?
                .map(|command| Some(command.routed)),
            _ => Err(CommandContextError::MultipleCommands),
        }
    }
}

struct IssuedCommand {
    aggregate_id: AggregateId,
    command: Box<dyn Any + Send + Sync>,
    routed: RoutedAggregateCommand,
}

enum CommandContextError {
    Encoding(String),
    InvalidCommand(RoutedAggregateCommandError),
    MultipleCommands,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedIntegrationCommand {
    receipt: CommandBusReceipt,
}

impl CompletedIntegrationCommand {
    pub const fn command_message_id(&self) -> &rostfrei_messaging_core::MessageId {
        self.receipt.response().command_message_id()
    }

    pub const fn publication_duplicate(&self) -> bool {
        self.receipt.publication_duplicate()
    }

    pub const fn response(&self) -> &rostfrei_messaging_core::CommandResponse {
        self.receipt.response()
    }

    pub fn into_response(self) -> rostfrei_messaging_core::CommandResponse {
        self.receipt.into_response()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntegrationEventOutcome {
    NoCommand,
    Completed(Box<CompletedIntegrationCommand>),
}

impl IntegrationEventOutcome {
    pub const fn command_message_id(&self) -> Option<&rostfrei_messaging_core::MessageId> {
        match self {
            Self::NoCommand => None,
            Self::Completed(completed) => Some(completed.command_message_id()),
        }
    }

    pub const fn publication_duplicate(&self) -> Option<bool> {
        match self {
            Self::NoCommand => None,
            Self::Completed(completed) => Some(completed.publication_duplicate()),
        }
    }

    pub const fn response(&self) -> Option<&rostfrei_messaging_core::CommandResponse> {
        match self {
            Self::NoCommand => None,
            Self::Completed(completed) => Some(completed.response()),
        }
    }
}

#[derive(Debug, Error)]
pub enum IntegrationEventProcessingError<HandlerError> {
    #[error("integration event handler failed")]
    Handler(HandlerError),
    #[error("integration event handler issued more than one command")]
    MultipleCommands,
    #[error("issued command could not be encoded: {message}")]
    CommandEncoding { message: String },
    #[error(transparent)]
    InvalidCommand(RoutedAggregateCommandError),
    #[error("deterministic operation identity could not be built: {0}")]
    MessageIdentity(String),
    #[error("command dispatch failed: {0}")]
    CommandBus(CommandBusError),
}

/// Dispatches commands produced by an incoming integration-event mapper.
pub struct IntegrationEventProcessor<Handler> {
    command_bus: CommandBus,
    durable_name: DurableName,
    handler: Handler,
}

impl<Handler> IntegrationEventProcessor<Handler> {
    pub const fn new(command_bus: CommandBus, durable_name: DurableName, handler: Handler) -> Self {
        Self {
            command_bus,
            durable_name,
            handler,
        }
    }

    pub async fn process<E>(
        &self,
        envelope: &IntegrationEventEnvelope<E>,
    ) -> Result<IntegrationEventOutcome, IntegrationEventProcessingError<Handler::Error>>
    where
        Handler: IntegrationEventHandler<E>,
        E: Sync,
    {
        let mut commands = CommandContext::new();
        self.handler
            .handle(envelope.payload(), &mut commands)
            .map_err(IntegrationEventProcessingError::Handler)?;
        let Some(command) = commands.into_command().map_err(|error| match error {
            CommandContextError::Encoding(message) => {
                IntegrationEventProcessingError::CommandEncoding { message }
            }
            CommandContextError::InvalidCommand(error) => {
                IntegrationEventProcessingError::InvalidCommand(error)
            }
            CommandContextError::MultipleCommands => {
                IntegrationEventProcessingError::MultipleCommands
            }
        })?
        else {
            return Ok(IntegrationEventOutcome::NoCommand);
        };

        let operation_id = integration_operation_id(
            &self.durable_name,
            envelope.message_id(),
            command.aggregate_type(),
            command.aggregate_id(),
        )
        .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let causation_id = CausationId::new(envelope.message_id().as_str())
            .map_err(|error| IntegrationEventProcessingError::MessageIdentity(error.to_string()))?;
        let request = DynamicCommandRequest::new(
            operation_id,
            command.aggregate_type(),
            AggregateId::new(command.aggregate_id()).map_err(|error| {
                IntegrationEventProcessingError::CommandBus(CommandBusError::new(
                    crate::CommandBusErrorKind::Encoding,
                    error.to_string(),
                ))
            })?,
            command.command(),
            command.schema_version(),
            command.payload().clone(),
        )
        .map_err(IntegrationEventProcessingError::CommandBus)?
        .with_correlation_id(envelope.correlation_id().clone())
        .with_causation_id(causation_id)
        .with_created_at(envelope.occurred_at());
        let receipt = self
            .command_bus
            .dispatch_dynamic(request)
            .await
            .map_err(IntegrationEventProcessingError::CommandBus)?;
        Ok(IntegrationEventOutcome::Completed(Box::new(
            CompletedIntegrationCommand { receipt },
        )))
    }
}

fn integration_operation_id(
    durable_name: &DurableName,
    source_message_id: &rostfrei_messaging_core::MessageId,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<OperationId, rostfrei_core::IdentityError> {
    let fingerprint = framed_fingerprint(&[
        b"rostfrei:integration-operation:v1",
        durable_name.as_str().as_bytes(),
        source_message_id.as_str().as_bytes(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
    ]);
    OperationId::new(format!("integration:{}", fingerprint.to_hex()))
}
