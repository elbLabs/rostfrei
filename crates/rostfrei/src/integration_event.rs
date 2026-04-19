use std::{any::Any, sync::Arc, time::Duration};

use rostfrei_core::{Aggregate, AggregateId, AggregateType, ContentFingerprint, IdentityError};
use rostfrei_messaging_core::{
    COMMAND_RESPONSE_SCHEMA_VERSION, CommandAddress, CommandEnvelope, CommandPublisher,
    CommandResponse, CommandResponseAddress, CommandResponseReadError,
    CommandResponseReadErrorKind, CommandResponseReader, ContractError, DurableName,
    EnvelopeContext, IntegrationEventEnvelope, MessageBuildError, MessageId, OperationId,
    OutboundMessage, PublishError, SchemaVersion, derive_command_response_address,
};
use rostfrei_registry::CommandDefinition;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::Value;
use thiserror::Error;
use tokio::time::sleep;

const RESPONSE_READ_SLICE: Duration = Duration::from_secs(1);
const RESPONSE_UNAVAILABLE_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Maps one integration event to at most one aggregate command.
pub trait IntegrationEventHandler<E>: Send + Sync {
    type Error;

    fn handle(&self, event: &E, commands: &mut CommandContext) -> Result<(), Self::Error>;
}

/// The transport payload used to route a command to an aggregate command worker.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoutedAggregateCommand {
    aggregate_type: String,
    aggregate_id: String,
    command: String,
    schema_version: u32,
    payload: Value,
}

impl RoutedAggregateCommand {
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        command: impl Into<String>,
        schema_version: u32,
        payload: Value,
    ) -> Result<Self, RoutedAggregateCommandError> {
        let aggregate_type = aggregate_type.into();
        let aggregate_id = aggregate_id.into();
        let command = command.into();
        AggregateType::new(aggregate_type.clone())
            .map_err(RoutedAggregateCommandError::AggregateType)?;
        AggregateId::new(aggregate_id.clone()).map_err(RoutedAggregateCommandError::AggregateId)?;
        CommandAddress::new("rostfrei", "routed-command", &command)
            .map_err(RoutedAggregateCommandError::CommandName)?;
        SchemaVersion::new(schema_version).map_err(RoutedAggregateCommandError::SchemaVersion)?;
        Ok(Self {
            aggregate_type,
            aggregate_id,
            command,
            schema_version,
            payload,
        })
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

#[derive(Deserialize)]
struct RoutedAggregateCommandWire {
    aggregate_type: String,
    aggregate_id: String,
    command: String,
    schema_version: u32,
    payload: Value,
}

impl<'de> Deserialize<'de> for RoutedAggregateCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RoutedAggregateCommandWire::deserialize(deserializer)?;
        Self::new(
            wire.aggregate_type,
            wire.aggregate_id,
            wire.command,
            wire.schema_version,
            wire.payload,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RoutedAggregateCommandError {
    #[error("invalid routed aggregate type: {0}")]
    AggregateType(IdentityError),
    #[error("invalid routed aggregate ID: {0}")]
    AggregateId(IdentityError),
    #[error("invalid routed command name: {0}")]
    CommandName(ContractError),
    #[error("invalid routed command schema version: {0}")]
    SchemaVersion(ContractError),
}

/// Collects the command issued while an integration event is being mapped.
///
/// Calling [`Self::issue`] performs no I/O. The processor publishes the command
/// only after the handler returns successfully and exactly one command was issued.
#[derive(Default)]
pub struct CommandContext {
    issued_count: usize,
    command: Option<Result<IssuedCommand, CommandContextError>>,
}

impl CommandContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue<Command>(&mut self, aggregate_id: AggregateId, command: Command)
    where
        Command: CommandDefinition + Serialize,
    {
        self.issued_count = self.issued_count.saturating_add(1);
        if self.issued_count != 1 {
            return;
        }

        let routed = serde_json::to_value(&command)
            .map_err(|error| CommandContextError::Encoding(error.to_string()))
            .and_then(|payload| {
                RoutedAggregateCommand::new(
                    <Command::Aggregate as Aggregate>::aggregate_type().into_owned(),
                    aggregate_id.as_str().to_owned(),
                    Command::COMMAND_NAME,
                    Command::SCHEMA_VERSION,
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

    /// Returns a typed view of the issued command for focused handler tests.
    pub fn issued<Command>(&self) -> Option<(&AggregateId, &Command)>
    where
        Command: 'static,
    {
        if self.issued_count != 1 {
            return None;
        }
        let command = self.command.as_ref()?.as_ref().ok()?;
        Some((
            &command.aggregate_id,
            command.command.downcast_ref::<Command>()?,
        ))
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

#[derive(Debug)]
enum CommandContextError {
    Encoding(String),
    InvalidCommand(RoutedAggregateCommandError),
    MultipleCommands,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedIntegrationCommand {
    command_message_id: MessageId,
    publication_duplicate: bool,
    response: CommandResponse,
}

impl CompletedIntegrationCommand {
    pub const fn command_message_id(&self) -> &MessageId {
        &self.command_message_id
    }

    pub const fn publication_duplicate(&self) -> bool {
        self.publication_duplicate
    }

    pub const fn response(&self) -> &CommandResponse {
        &self.response
    }

    pub fn into_response(self) -> CommandResponse {
        self.response
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntegrationEventOutcome {
    NoCommand,
    Completed(Box<CompletedIntegrationCommand>),
}

impl IntegrationEventOutcome {
    pub const fn command_message_id(&self) -> Option<&MessageId> {
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

    pub const fn response(&self) -> Option<&CommandResponse> {
        match self {
            Self::NoCommand => None,
            Self::Completed(completed) => Some(completed.response()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InvalidCommandResponse {
    #[error("response command address does not match the published command")]
    CommandAddress,
    #[error("response command message ID does not match the published command")]
    CommandMessageId,
    #[error("response operation ID does not match the published command")]
    OperationId,
    #[error("response schema version is not the command-response schema version")]
    SchemaVersion,
    #[error("response correlation ID does not match the integration event")]
    CorrelationId,
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
    #[error("issued command `{issued}` does not match configured route `{configured}`")]
    RouteMismatch { configured: String, issued: String },
    #[error("deterministic command identity could not be built: {0}")]
    MessageIdentity(ContractError),
    #[error("command envelope could not be built: {0}")]
    MessageBuild(MessageBuildError),
    #[error("command publication failed: {0}")]
    Publish(PublishError),
    #[error("command response address could not be derived: {0}")]
    ResponseAddress(ContractError),
    #[error("command response read failed: {0}")]
    ResponseRead(CommandResponseReadError),
    #[error(transparent)]
    InvalidResponse(InvalidCommandResponse),
}

/// Publishes commands produced by one integration-event handler and waits for their response.
pub struct IntegrationEventProcessor<Handler> {
    publisher: Arc<dyn CommandPublisher>,
    response_reader: Arc<dyn CommandResponseReader>,
    command_address: CommandAddress,
    durable_name: DurableName,
    handler: Handler,
}

impl<Handler> IntegrationEventProcessor<Handler> {
    pub fn new(
        publisher: Arc<dyn CommandPublisher>,
        response_reader: Arc<dyn CommandResponseReader>,
        command_address: CommandAddress,
        durable_name: DurableName,
        handler: Handler,
    ) -> Self {
        Self {
            publisher,
            response_reader,
            command_address,
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

        if command.command() != self.command_address.name() {
            return Err(IntegrationEventProcessingError::RouteMismatch {
                configured: self.command_address.as_str().to_owned(),
                issued: command.command().to_owned(),
            });
        }

        let operation_id = integration_operation_id(
            &self.durable_name,
            envelope.message_id(),
            command.aggregate_type(),
            command.aggregate_id(),
        )
        .map_err(IntegrationEventProcessingError::MessageIdentity)?;
        let command_fingerprint =
            command_fingerprint(&self.command_address, &command).map_err(|error| {
                IntegrationEventProcessingError::CommandEncoding {
                    message: error.to_string(),
                }
            })?;
        let command_message_id = command_message_id(&operation_id, command_fingerprint)
            .map_err(IntegrationEventProcessingError::MessageIdentity)?;
        let command_schema = SchemaVersion::new(command.schema_version())
            .map_err(IntegrationEventProcessingError::MessageIdentity)?;
        let command_envelope = CommandEnvelope::new(
            EnvelopeContext::new(
                command_message_id.clone(),
                command_schema,
                envelope.correlation_id().clone(),
                Some(envelope.message_id().into()),
            ),
            operation_id.clone(),
            envelope.occurred_at(),
            command,
        )
        .map_err(IntegrationEventProcessingError::MessageBuild)?;
        let outbound = OutboundMessage::json(
            self.command_address.clone(),
            command_message_id.clone(),
            &command_envelope,
        )
        .map_err(IntegrationEventProcessingError::MessageBuild)?;
        let publication = self
            .publisher
            .publish_command(outbound)
            .await
            .map_err(IntegrationEventProcessingError::Publish)?;

        let response_address = derive_command_response_address(
            &self.command_address,
            &operation_id,
            &command_message_id,
        )
        .map_err(IntegrationEventProcessingError::ResponseAddress)?;
        let response = self
            .read_response(&response_address, &operation_id, &command_message_id)
            .await
            .map_err(IntegrationEventProcessingError::ResponseRead)?;

        validate_response(
            &response,
            &self.command_address,
            &operation_id,
            &command_message_id,
            envelope.correlation_id(),
        )
        .map_err(IntegrationEventProcessingError::InvalidResponse)?;

        Ok(IntegrationEventOutcome::Completed(Box::new(
            CompletedIntegrationCommand {
                command_message_id,
                publication_duplicate: publication.duplicate(),
                response,
            },
        )))
    }

    async fn read_response(
        &self,
        response_address: &CommandResponseAddress,
        operation_id: &OperationId,
        command_message_id: &MessageId,
    ) -> Result<CommandResponse, CommandResponseReadError>
    where
        Handler: Sync,
    {
        loop {
            match self
                .response_reader
                .read_command_response(
                    response_address,
                    operation_id,
                    command_message_id,
                    RESPONSE_READ_SLICE,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(error) if error.kind() == CommandResponseReadErrorKind::Timeout => {}
                Err(error) if error.kind() == CommandResponseReadErrorKind::Unavailable => {
                    sleep(RESPONSE_UNAVAILABLE_RETRY_DELAY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn integration_operation_id(
    durable_name: &DurableName,
    source_message_id: &MessageId,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<OperationId, ContractError> {
    let fingerprint = framed_fingerprint(&[
        b"rostfrei:integration-operation:v1",
        durable_name.as_str().as_bytes(),
        source_message_id.as_str().as_bytes(),
        aggregate_type.as_bytes(),
        aggregate_id.as_bytes(),
    ]);
    OperationId::new(format!("integration:{}", fingerprint.to_hex()))
}

fn command_fingerprint(
    command_address: &CommandAddress,
    command: &RoutedAggregateCommand,
) -> Result<ContentFingerprint, serde_json::Error> {
    let schema_version = command.schema_version().to_be_bytes();
    let payload = serde_json::to_vec(command.payload())?;
    Ok(framed_fingerprint(&[
        b"rostfrei:integration-dispatch-request:v2",
        command_address.as_str().as_bytes(),
        command.aggregate_type().as_bytes(),
        command.aggregate_id().as_bytes(),
        command.command().as_bytes(),
        &schema_version,
        &payload,
    ]))
}

fn command_message_id(
    operation_id: &OperationId,
    command_fingerprint: ContentFingerprint,
) -> Result<MessageId, ContractError> {
    let identity = format!(
        "rostfrei:dispatch-message:v1:{}:{}",
        operation_id.as_str(),
        command_fingerprint.to_hex()
    );
    MessageId::new(ContentFingerprint::digest(identity).to_hex())
}

fn validate_response(
    response: &CommandResponse,
    command_address: &CommandAddress,
    operation_id: &OperationId,
    command_message_id: &MessageId,
    correlation_id: &rostfrei_messaging_core::CorrelationId,
) -> Result<(), InvalidCommandResponse> {
    if response.command_address() != command_address {
        return Err(InvalidCommandResponse::CommandAddress);
    }
    if response.command_message_id() != command_message_id {
        return Err(InvalidCommandResponse::CommandMessageId);
    }
    if response.operation_id() != operation_id {
        return Err(InvalidCommandResponse::OperationId);
    }
    if response.schema_version().get() != COMMAND_RESPONSE_SCHEMA_VERSION {
        return Err(InvalidCommandResponse::SchemaVersion);
    }
    if response.correlation_id() != correlation_id {
        return Err(InvalidCommandResponse::CorrelationId);
    }
    Ok(())
}

fn framed_fingerprint(parts: &[&[u8]]) -> ContentFingerprint {
    let mut framed = Vec::new();
    for part in parts {
        framed.extend_from_slice(&bounded_length_bytes(part.len()));
        framed.extend_from_slice(part);
    }
    ContentFingerprint::digest(framed)
}

fn bounded_length_bytes(length: usize) -> [u8; 8] {
    let mut encoded = [0_u8; 8];
    for (target, source) in encoded
        .iter_mut()
        .rev()
        .zip(length.to_be_bytes().iter().rev())
    {
        *target = *source;
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, convert::Infallible, sync::Mutex};

    use async_trait::async_trait;
    use rostfrei_messaging_core::{
        ApplicationErrorCode, CommandRejection, CommandRejectionClassification,
        CommandResponseAddress, CommandResponseOutcome, CorrelationId, MessageTimestamp,
        OutboundMessage, PublishReceipt,
    };

    use super::*;

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct OrderPlaced {
        order_id: String,
        quantity: u32,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
    struct ReserveInventory {
        order_id: String,
        quantity: u32,
    }

    struct Inventory;

    impl Aggregate for Inventory {
        type State = ();
        type Event = ();

        const AGGREGATE_TYPE: &'static str = "inventory";

        fn initial(_stream_id: &rostfrei_core::StreamId) -> Self::State {}

        fn apply(_state: &mut Self::State, _event: &Self::Event) {}
    }

    impl CommandDefinition for ReserveInventory {
        type Aggregate = Inventory;

        const COMMAND_NAME: &'static str = "reserve-inventory";
        const SCHEMA_VERSION: u32 = 2;
    }

    #[derive(Serialize)]
    struct ReleaseInventory;

    impl CommandDefinition for ReleaseInventory {
        type Aggregate = Inventory;

        const COMMAND_NAME: &'static str = "release-inventory";
        const SCHEMA_VERSION: u32 = 2;
    }

    struct ReserveInventoryWhenOrderPlaced;

    impl IntegrationEventHandler<OrderPlaced> for ReserveInventoryWhenOrderPlaced {
        type Error = Infallible;

        fn handle(
            &self,
            event: &OrderPlaced,
            commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            commands.issue(
                AggregateId::new(&event.order_id).unwrap(),
                ReserveInventory {
                    order_id: event.order_id.clone(),
                    quantity: event.quantity,
                },
            );
            Ok(())
        }
    }

    struct IgnoreOrderPlaced;

    impl IntegrationEventHandler<OrderPlaced> for IgnoreOrderPlaced {
        type Error = Infallible;

        fn handle(
            &self,
            _event: &OrderPlaced,
            _commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct IssueTwice;

    impl IntegrationEventHandler<OrderPlaced> for IssueTwice {
        type Error = Infallible;

        fn handle(
            &self,
            event: &OrderPlaced,
            commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            for _ in 0..2 {
                commands.issue(
                    AggregateId::new(&event.order_id).unwrap(),
                    ReserveInventory {
                        order_id: event.order_id.clone(),
                        quantity: event.quantity,
                    },
                );
            }
            Ok(())
        }
    }

    struct FailAfterIssue;

    impl IntegrationEventHandler<OrderPlaced> for FailAfterIssue {
        type Error = &'static str;

        fn handle(
            &self,
            event: &OrderPlaced,
            commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            commands.issue(
                AggregateId::new(&event.order_id).unwrap(),
                ReserveInventory {
                    order_id: event.order_id.clone(),
                    quantity: event.quantity,
                },
            );
            Err("invalid mapping")
        }
    }

    struct MisdirectOrderPlaced;

    impl IntegrationEventHandler<OrderPlaced> for MisdirectOrderPlaced {
        type Error = Infallible;

        fn handle(
            &self,
            event: &OrderPlaced,
            commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            commands.issue(AggregateId::new(&event.order_id).unwrap(), ReleaseInventory);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeCommandPublisher {
        messages: Mutex<Vec<OutboundMessage<CommandAddress>>>,
        receipts: Mutex<VecDeque<PublishReceipt>>,
    }

    impl FakeCommandPublisher {
        fn with_receipts(receipts: impl IntoIterator<Item = PublishReceipt>) -> Self {
            Self {
                messages: Mutex::new(Vec::new()),
                receipts: Mutex::new(receipts.into_iter().collect()),
            }
        }

        fn messages(&self) -> Vec<OutboundMessage<CommandAddress>> {
            self.messages.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommandPublisher for FakeCommandPublisher {
        async fn publish_command(
            &self,
            message: OutboundMessage<CommandAddress>,
        ) -> Result<PublishReceipt, PublishError> {
            self.messages.lock().unwrap().push(message);
            Ok(self
                .receipts
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(PublishReceipt::new(false)))
        }
    }

    enum ResponseAction {
        Timeout,
        Unavailable,
        Accepted,
        Rejected(CommandRejection),
        WrongAddress,
    }

    #[derive(Clone, Debug)]
    struct ResponseRead {
        address: CommandResponseAddress,
        operation_id: OperationId,
        command_message_id: MessageId,
        timeout: Duration,
    }

    struct FakeCommandResponseReader {
        command_address: CommandAddress,
        correlation_id: CorrelationId,
        actions: Mutex<VecDeque<ResponseAction>>,
        stored: Mutex<Option<CommandResponse>>,
        reads: Mutex<Vec<ResponseRead>>,
    }

    impl FakeCommandResponseReader {
        fn new(actions: impl IntoIterator<Item = ResponseAction>) -> Self {
            Self {
                command_address: command_address(),
                correlation_id: CorrelationId::new("checkout-42").unwrap(),
                actions: Mutex::new(actions.into_iter().collect()),
                stored: Mutex::new(None),
                reads: Mutex::new(Vec::new()),
            }
        }

        fn reads(&self) -> Vec<ResponseRead> {
            self.reads.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CommandResponseReader for FakeCommandResponseReader {
        async fn read_command_response(
            &self,
            address: &CommandResponseAddress,
            expected_operation_id: &OperationId,
            expected_command_message_id: &MessageId,
            timeout: Duration,
        ) -> Result<CommandResponse, CommandResponseReadError> {
            let mut reads = self.reads.lock().unwrap();
            reads.push(ResponseRead {
                address: address.clone(),
                operation_id: expected_operation_id.clone(),
                command_message_id: expected_command_message_id.clone(),
                timeout,
            });
            let response_number = reads.len();
            drop(reads);
            let stored = self.stored.lock().unwrap().clone();
            if let Some(response) = stored {
                return Ok(response);
            }
            let action = self.actions.lock().unwrap().pop_front().unwrap();
            let response = match action {
                ResponseAction::Timeout => {
                    return Err(CommandResponseReadError::new(
                        CommandResponseReadErrorKind::Timeout,
                    ));
                }
                ResponseAction::Unavailable => {
                    return Err(CommandResponseReadError::new(
                        CommandResponseReadErrorKind::Unavailable,
                    ));
                }
                ResponseAction::Accepted => CommandResponse::accepted(
                    MessageId::new(format!("response-{response_number}")).unwrap(),
                    expected_command_message_id.clone(),
                    self.command_address.clone(),
                    expected_operation_id.clone(),
                    self.correlation_id.clone(),
                ),
                ResponseAction::Rejected(rejection) => CommandResponse::rejected(
                    MessageId::new(format!("response-{response_number}")).unwrap(),
                    expected_command_message_id.clone(),
                    self.command_address.clone(),
                    expected_operation_id.clone(),
                    self.correlation_id.clone(),
                    rejection,
                ),
                ResponseAction::WrongAddress => CommandResponse::accepted(
                    MessageId::new(format!("response-{response_number}")).unwrap(),
                    expected_command_message_id.clone(),
                    CommandAddress::new("shop", "inventory", "release-inventory").unwrap(),
                    expected_operation_id.clone(),
                    self.correlation_id.clone(),
                ),
            };
            let response = response.map_err(|_| {
                CommandResponseReadError::new(CommandResponseReadErrorKind::InvalidResponse)
            })?;
            *self.stored.lock().unwrap() = Some(response.clone());
            Ok(response)
        }
    }

    fn envelope() -> IntegrationEventEnvelope<OrderPlaced> {
        IntegrationEventEnvelope::new(
            EnvelopeContext::new(
                MessageId::new("order-placed-42").unwrap(),
                SchemaVersion::new(1).unwrap(),
                CorrelationId::new("checkout-42").unwrap(),
                None,
            ),
            MessageTimestamp::from_unix_milliseconds(1_700_000_000_123).unwrap(),
            OrderPlaced {
                order_id: "order-42".to_owned(),
                quantity: 3,
            },
        )
        .unwrap()
    }

    fn command_address() -> CommandAddress {
        CommandAddress::new("shop", "inventory", "reserve-inventory").unwrap()
    }

    fn durable() -> DurableName {
        DurableName::new("shop", "orders", "reserve-inventory", 1).unwrap()
    }

    fn processor<Handler>(
        publisher: Arc<FakeCommandPublisher>,
        reader: Arc<FakeCommandResponseReader>,
        handler: Handler,
    ) -> IntegrationEventProcessor<Handler> {
        IntegrationEventProcessor::new(publisher, reader, command_address(), durable(), handler)
    }

    #[test]
    fn command_context_exposes_the_transport_neutral_mapping() {
        let mut commands = CommandContext::new();
        ReserveInventoryWhenOrderPlaced
            .handle(envelope().payload(), &mut commands)
            .unwrap();

        let command = commands.issued_command().unwrap();
        assert_eq!(command.aggregate_type(), "inventory");
        assert_eq!(command.aggregate_id(), "order-42");
        assert_eq!(command.command(), "reserve-inventory");
        assert_eq!(command.schema_version(), 2);
        assert_eq!(command.payload()["quantity"], 3);

        let (aggregate_id, typed) = commands.issued::<ReserveInventory>().unwrap();
        assert_eq!(aggregate_id.as_str(), "order-42");
        assert_eq!(typed.quantity, 3);
    }

    #[test]
    fn routed_command_deserialization_revalidates_identity_and_schema() {
        let invalid = serde_json::json!({
            "aggregate_type": " inventory",
            "aggregate_id": "order-42",
            "command": "reserve-inventory",
            "schema_version": 2,
            "payload": {}
        });
        assert!(serde_json::from_value::<RoutedAggregateCommand>(invalid).is_err());

        let invalid = serde_json::json!({
            "aggregate_type": "inventory",
            "aggregate_id": "order-42",
            "command": "reserve-inventory",
            "schema_version": 0,
            "payload": {}
        });
        assert!(serde_json::from_value::<RoutedAggregateCommand>(invalid).is_err());
    }

    #[test]
    fn command_identity_includes_the_full_destination_address() {
        let command = RoutedAggregateCommand::new(
            "inventory",
            "order-42",
            "reserve-inventory",
            2,
            serde_json::json!({"quantity": 3}),
        )
        .unwrap();
        let operation_id = OperationId::new("integration-operation").unwrap();
        let first = command_message_id(
            &operation_id,
            command_fingerprint(&command_address(), &command).unwrap(),
        )
        .unwrap();
        let corrected_route =
            CommandAddress::new("shop", "warehouse", "reserve-inventory").unwrap();
        let second = command_message_id(
            &operation_id,
            command_fingerprint(&corrected_route, &command).unwrap(),
        )
        .unwrap();

        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn published_envelope_has_the_routed_command_and_deterministic_context() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([ResponseAction::Accepted]));
        let processor = processor(
            Arc::clone(&publisher),
            Arc::clone(&reader),
            ReserveInventoryWhenOrderPlaced,
        );

        let outcome = processor.process(&envelope()).await.unwrap();
        let messages = publisher.messages();
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.address(), &command_address());
        assert_eq!(message.message_id(), outcome.command_message_id().unwrap());
        let emitted: CommandEnvelope<RoutedAggregateCommand> =
            serde_json::from_slice(message.payload()).unwrap();
        assert_eq!(emitted.message_id(), message.message_id());
        assert_eq!(
            emitted.operation_id().as_str(),
            "integration:fa325e0a9963322bd3f15c1e74d0e47aa7d76fd291fe42a976d061882a8df212"
        );
        assert_eq!(
            emitted.message_id().as_str(),
            "fab276340f4e87a4c39465a043960cdde663dc11f88af492b800fe4953107d7c"
        );
        assert_eq!(emitted.schema_version().get(), 2);
        assert_eq!(emitted.created_at(), envelope().occurred_at());
        assert_eq!(emitted.correlation_id().as_str(), "checkout-42");
        assert_eq!(emitted.causation_id().unwrap().as_str(), "order-placed-42");
        assert_eq!(emitted.payload().aggregate_type(), "inventory");
        assert_eq!(emitted.payload().aggregate_id(), "order-42");
        assert_eq!(emitted.payload().command(), "reserve-inventory");
        assert_eq!(emitted.payload().schema_version(), 2);
        assert_eq!(emitted.payload().payload()["quantity"], 3);

        let read = &reader.reads()[0];
        assert_eq!(read.operation_id, *emitted.operation_id());
        assert_eq!(read.command_message_id, *emitted.message_id());
        assert_eq!(read.timeout, RESPONSE_READ_SLICE);
        assert_eq!(
            read.address,
            derive_command_response_address(
                message.address(),
                emitted.operation_id(),
                emitted.message_id(),
            )
            .unwrap()
        );
    }

    #[tokio::test]
    async fn no_multiple_and_failed_mappings_publish_nothing() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([]));

        let no_command = processor(
            Arc::clone(&publisher),
            Arc::clone(&reader),
            IgnoreOrderPlaced,
        )
        .process(&envelope())
        .await
        .unwrap();
        assert_eq!(no_command, IntegrationEventOutcome::NoCommand);

        let multiple = processor(Arc::clone(&publisher), Arc::clone(&reader), IssueTwice)
            .process(&envelope())
            .await;
        assert!(matches!(
            multiple,
            Err(IntegrationEventProcessingError::MultipleCommands)
        ));

        let failed = processor(Arc::clone(&publisher), Arc::clone(&reader), FailAfterIssue)
            .process(&envelope())
            .await;
        assert!(matches!(
            failed,
            Err(IntegrationEventProcessingError::Handler("invalid mapping"))
        ));
        assert!(publisher.messages().is_empty());
        assert!(reader.reads().is_empty());
    }

    #[tokio::test]
    async fn command_name_must_match_the_configured_route() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([]));
        let result = processor(
            Arc::clone(&publisher),
            Arc::clone(&reader),
            MisdirectOrderPlaced,
        )
        .process(&envelope())
        .await;

        assert!(matches!(
            result,
            Err(IntegrationEventProcessingError::RouteMismatch { .. })
        ));
        assert!(publisher.messages().is_empty());
        assert!(reader.reads().is_empty());
    }

    #[tokio::test]
    async fn response_timeouts_are_sliced_until_an_accepted_response_arrives() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([
            ResponseAction::Timeout,
            ResponseAction::Accepted,
        ]));
        let outcome = processor(
            publisher,
            Arc::clone(&reader),
            ReserveInventoryWhenOrderPlaced,
        )
        .process(&envelope())
        .await
        .unwrap();

        assert_eq!(reader.reads().len(), 2);
        assert_eq!(
            outcome.response().unwrap().outcome(),
            &CommandResponseOutcome::Accepted
        );
        assert_eq!(outcome.publication_duplicate(), Some(false));
    }

    #[tokio::test]
    async fn unavailable_response_reads_are_backed_off_before_retrying() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([
            ResponseAction::Unavailable,
            ResponseAction::Accepted,
        ]));
        let started = tokio::time::Instant::now();

        processor(
            publisher,
            Arc::clone(&reader),
            ReserveInventoryWhenOrderPlaced,
        )
        .process(&envelope())
        .await
        .unwrap();

        assert_eq!(reader.reads().len(), 2);
        assert!(started.elapsed() >= RESPONSE_UNAVAILABLE_RETRY_DELAY);
    }

    #[tokio::test]
    async fn rejected_response_preserves_business_classification_code_and_details() {
        let rejection = CommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("inventory.insufficient").unwrap(),
            "inventory is unavailable",
            Some(serde_json::json!({"sku": "bike-42", "available": 0})),
        )
        .unwrap();
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([ResponseAction::Rejected(
            rejection,
        )]));
        let outcome = processor(publisher, reader, ReserveInventoryWhenOrderPlaced)
            .process(&envelope())
            .await
            .unwrap();

        let CommandResponseOutcome::Rejected(rejection) = outcome.response().unwrap().outcome()
        else {
            panic!("expected rejection");
        };
        assert_eq!(
            rejection.classification(),
            CommandRejectionClassification::Conflict
        );
        assert_eq!(rejection.code().as_str(), "inventory.insufficient");
        assert_eq!(rejection.details().unwrap()["available"], 0);
    }

    #[tokio::test]
    async fn redelivery_republishes_exact_identity_and_exposes_duplicate_ack() {
        let publisher = Arc::new(FakeCommandPublisher::with_receipts([
            PublishReceipt::new(false),
            PublishReceipt::new(true),
        ]));
        let reader = Arc::new(FakeCommandResponseReader::new([ResponseAction::Accepted]));
        let processor = processor(
            Arc::clone(&publisher),
            Arc::clone(&reader),
            ReserveInventoryWhenOrderPlaced,
        );
        let source = envelope();

        let first = processor.process(&source).await.unwrap();
        let second = processor.process(&source).await.unwrap();

        let messages = publisher.messages();
        assert_eq!(messages[0], messages[1]);
        assert_eq!(first.command_message_id(), second.command_message_id());
        assert_eq!(first.response(), second.response());
        assert_eq!(first.publication_duplicate(), Some(false));
        assert_eq!(second.publication_duplicate(), Some(true));
        assert_eq!(reader.reads().len(), 2);
    }

    #[tokio::test]
    async fn invalid_response_identity_is_terminal() {
        let publisher = Arc::new(FakeCommandPublisher::default());
        let reader = Arc::new(FakeCommandResponseReader::new([
            ResponseAction::WrongAddress,
        ]));
        let result = processor(publisher, reader, ReserveInventoryWhenOrderPlaced)
            .process(&envelope())
            .await;

        assert!(matches!(
            result,
            Err(IntegrationEventProcessingError::InvalidResponse(
                InvalidCommandResponse::CommandAddress
            ))
        ));
    }
}
