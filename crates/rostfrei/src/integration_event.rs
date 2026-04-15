use std::{any::Any, fmt, sync::Arc};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandExecutionError, CommandHandler, CommandOutcome,
    CommandReceipt, ContentFingerprint, Event, EventCodecError, EventStore, EventStoreError,
    ExecutionMetadata, Executor, IdentityError, OperationId, StreamId,
};
use rostfrei_messaging_core::{DurableName, IntegrationEventEnvelope};
use rostfrei_registry::CommandDefinition;
use serde::Serialize;
use thiserror::Error;

/// Maps one integration event to at most one local aggregate command.
pub trait IntegrationEventHandler<E>: Send + Sync {
    type Error;

    fn handle(&self, event: &E, commands: &mut CommandContext) -> Result<(), Self::Error>;
}

/// Collects the command issued while an integration event is being handled.
///
/// Calling [`Self::issue`] performs no I/O. The processor executes the command
/// only after the handler returns successfully. A handler may issue zero or one
/// command; issuing more than one is rejected before either command executes.
#[derive(Default)]
pub struct CommandContext {
    issued_count: usize,
    command: Option<Result<Box<dyn IssuedCommand>, String>>,
}

impl CommandContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue<Command>(&mut self, aggregate_id: AggregateId, command: Command)
    where
        Command: CommandDefinition + Serialize,
        Command::Aggregate: CommandHandler<Command>,
        <Command::Aggregate as Aggregate>::State: Send,
        <Command::Aggregate as Aggregate>::Event: Event + Send,
        <Command::Aggregate as CommandHandler<Command>>::Rejection: Send + Sync + 'static,
    {
        self.issued_count = self.issued_count.saturating_add(1);
        if self.issued_count != 1 {
            return;
        }

        self.command = Some(
            canonical_json(&command)
                .map(|payload| {
                    Box::new(TypedIssuedCommand {
                        aggregate_id,
                        command,
                        payload,
                    }) as Box<dyn IssuedCommand>
                })
                .map_err(|error| error.to_string()),
        );
    }

    pub const fn issued_count(&self) -> usize {
        self.issued_count
    }

    pub const fn is_empty(&self) -> bool {
        self.issued_count == 0
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
        Some((command.aggregate_id(), command.command().downcast_ref()?))
    }

    fn into_command(self) -> Result<Option<Box<dyn IssuedCommand>>, CommandContextError> {
        match self.issued_count {
            0 => Ok(None),
            1 => self
                .command
                .expect("one issued command always records its intent")
                .map(Some)
                .map_err(CommandContextError::Encoding),
            _ => Err(CommandContextError::MultipleCommands),
        }
    }
}

#[derive(Debug)]
enum CommandContextError {
    Encoding(String),
    MultipleCommands,
}

#[async_trait]
trait IssuedCommand: Send + Sync {
    fn aggregate_id(&self) -> &AggregateId;

    fn command(&self) -> &dyn Any;

    fn stream_id(&self) -> Result<StreamId, IdentityError>;

    fn fingerprint(&self, stream_id: &StreamId) -> ContentFingerprint;

    async fn execute(
        &self,
        store: Arc<dyn EventStore>,
        metadata: ExecutionMetadata,
    ) -> Result<IssuedCommandOutcome, CommandExecutionError>;
}

struct TypedIssuedCommand<Command> {
    aggregate_id: AggregateId,
    command: Command,
    payload: Vec<u8>,
}

#[async_trait]
impl<Command> IssuedCommand for TypedIssuedCommand<Command>
where
    Command: CommandDefinition + Serialize,
    Command::Aggregate: CommandHandler<Command>,
    <Command::Aggregate as Aggregate>::State: Send,
    <Command::Aggregate as Aggregate>::Event: Event + Send,
    <Command::Aggregate as CommandHandler<Command>>::Rejection: Send + Sync + 'static,
{
    fn aggregate_id(&self) -> &AggregateId {
        &self.aggregate_id
    }

    fn command(&self) -> &dyn Any {
        &self.command
    }

    fn stream_id(&self) -> Result<StreamId, IdentityError> {
        Ok(StreamId::new(
            AggregateType::new(Command::Aggregate::aggregate_type())?,
            self.aggregate_id.clone(),
        ))
    }

    fn fingerprint(&self, stream_id: &StreamId) -> ContentFingerprint {
        let schema_version = Command::SCHEMA_VERSION.to_be_bytes();
        framed_fingerprint(&[
            b"rostfrei:integration-command:v1",
            stream_id.aggregate_type().as_str().as_bytes(),
            stream_id.aggregate_id().as_str().as_bytes(),
            Command::COMMAND_NAME.as_bytes(),
            &schema_version,
            &self.payload,
        ])
    }

    async fn execute(
        &self,
        store: Arc<dyn EventStore>,
        metadata: ExecutionMetadata,
    ) -> Result<IssuedCommandOutcome, CommandExecutionError> {
        let outcome = Executor::new(store)
            .execute::<Command::Aggregate, Command>(metadata, &self.command)
            .await?;
        Ok(match outcome {
            CommandOutcome::Accepted(receipt) => IssuedCommandOutcome::Accepted(receipt),
            CommandOutcome::Rejected(rejection) => {
                IssuedCommandOutcome::Rejected(CommandRejection {
                    command_name: Command::COMMAND_NAME,
                    value: Box::new(rejection),
                })
            }
        })
    }
}

enum IssuedCommandOutcome {
    Accepted(CommandReceipt),
    Rejected(CommandRejection),
}

/// A local command rejection whose concrete value remains available by type.
pub struct CommandRejection {
    command_name: &'static str,
    value: Box<dyn Any + Send + Sync>,
}

impl CommandRejection {
    pub const fn command_name(&self) -> &'static str {
        self.command_name
    }

    pub fn value<Rejection>(&self) -> Option<&Rejection>
    where
        Rejection: 'static,
    {
        self.value.downcast_ref()
    }
}

impl fmt::Debug for CommandRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandRejection")
            .field("command_name", &self.command_name)
            .field("value_type_id", &(*self.value).type_id())
            .finish()
    }
}

impl fmt::Display for CommandRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "command `{}` was rejected", self.command_name)
    }
}

impl std::error::Error for CommandRejection {}

#[derive(Debug)]
pub enum IntegrationEventOutcome {
    NoCommand,
    Accepted(CommandReceipt),
    Rejected(CommandRejection),
}

impl IntegrationEventOutcome {
    pub const fn receipt(&self) -> Option<&CommandReceipt> {
        match self {
            Self::Accepted(receipt) => Some(receipt),
            Self::NoCommand | Self::Rejected(_) => None,
        }
    }

    pub const fn rejection(&self) -> Option<&CommandRejection> {
        match self {
            Self::Rejected(rejection) => Some(rejection),
            Self::NoCommand | Self::Accepted(_) => None,
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
    InvalidTarget(IdentityError),
    #[error(transparent)]
    Store(EventStoreError),
    #[error(transparent)]
    Codec(EventCodecError),
}

/// Executes commands produced by one integration-event handler.
pub struct IntegrationEventProcessor<Handler> {
    store: Arc<dyn EventStore>,
    durable_name: DurableName,
    handler: Handler,
}

impl<Handler> IntegrationEventProcessor<Handler> {
    pub fn new<Store>(store: Store, durable_name: DurableName, handler: Handler) -> Self
    where
        Store: EventStore + 'static,
    {
        Self {
            store: Arc::new(store),
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
    {
        let mut commands = CommandContext::new();
        self.handler
            .handle(envelope.payload(), &mut commands)
            .map_err(IntegrationEventProcessingError::Handler)?;
        let Some(command) = commands.into_command().map_err(|error| match error {
            CommandContextError::Encoding(message) => {
                IntegrationEventProcessingError::CommandEncoding { message }
            }
            CommandContextError::MultipleCommands => {
                IntegrationEventProcessingError::MultipleCommands
            }
        })?
        else {
            return Ok(IntegrationEventOutcome::NoCommand);
        };

        let stream_id = command
            .stream_id()
            .map_err(IntegrationEventProcessingError::InvalidTarget)?;
        let operation_id = integration_operation_id(
            &self.durable_name,
            envelope.message_id().as_str(),
            &stream_id,
        );
        let metadata = ExecutionMetadata::new(
            stream_id.clone(),
            operation_id,
            command.fingerprint(&stream_id),
        )
        .with_correlation_id(envelope.correlation_id().clone())
        .with_causation_id(envelope.message_id().into());

        command
            .execute(Arc::clone(&self.store), metadata)
            .await
            .map(|outcome| match outcome {
                IssuedCommandOutcome::Accepted(receipt) => {
                    IntegrationEventOutcome::Accepted(receipt)
                }
                IssuedCommandOutcome::Rejected(rejection) => {
                    IntegrationEventOutcome::Rejected(rejection)
                }
            })
            .map_err(|error| match error {
                CommandExecutionError::Store(error) => {
                    IntegrationEventProcessingError::Store(error)
                }
                CommandExecutionError::Codec(error) => {
                    IntegrationEventProcessingError::Codec(error)
                }
            })
    }
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_value(value).and_then(|value| serde_json::to_vec(&value))
}

fn integration_operation_id(
    durable_name: &DurableName,
    message_id: &str,
    stream_id: &StreamId,
) -> OperationId {
    let fingerprint = framed_fingerprint(&[
        b"rostfrei:integration-operation:v1",
        durable_name.as_str().as_bytes(),
        message_id.as_bytes(),
        stream_id.aggregate_type().as_str().as_bytes(),
        stream_id.aggregate_id().as_str().as_bytes(),
    ]);
    OperationId::new(format!("integration:{}", fingerprint.to_hex()))
        .expect("derived integration operation identities are always valid")
}

fn framed_fingerprint(parts: &[&[u8]]) -> ContentFingerprint {
    let mut framed = Vec::new();
    for part in parts {
        let length = u64::try_from(part.len()).expect("fingerprint parts fit in u64");
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(part);
    }
    ContentFingerprint::digest(framed)
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rostfrei_core::{AggregateInstance, CommandHandler, EventCodecErrorKind, RecordedEvent};
    use rostfrei_messaging_core::{
        CorrelationId, EnvelopeContext, MessageId, MessageTimestamp, SchemaVersion,
    };
    use serde::Deserialize;

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

    #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct InventoryReserved {
        order_id: String,
        quantity: u32,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct InventoryUnavailable {
        order_id: String,
    }

    struct Inventory;

    impl Aggregate for Inventory {
        type State = Vec<InventoryReserved>;
        type Event = InventoryReserved;

        const AGGREGATE_TYPE: &'static str = "inventory";

        fn initial(_stream_id: &StreamId) -> Self::State {
            Vec::new()
        }

        fn apply(state: &mut Self::State, event: &Self::Event) {
            state.push(event.clone());
        }
    }

    impl Event for InventoryReserved {
        fn event_type(&self) -> &'static str {
            "inventory-reserved"
        }

        fn schema_version(&self) -> u32 {
            1
        }

        fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
            serde_json::to_vec(self).map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
            })
        }

        fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
            serde_json::from_slice(event.payload()).map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
            })
        }
    }

    impl CommandHandler<ReserveInventory> for Inventory {
        type Rejection = InventoryUnavailable;

        fn handle(
            command: &ReserveInventory,
            aggregate: &mut AggregateInstance<Self>,
        ) -> Result<(), Self::Rejection> {
            if command.quantity == 0 {
                return Err(InventoryUnavailable {
                    order_id: command.order_id.clone(),
                });
            }
            aggregate.raise(InventoryReserved {
                order_id: command.order_id.clone(),
                quantity: command.quantity,
            });
            Ok(())
        }
    }

    impl CommandDefinition for ReserveInventory {
        type Aggregate = Inventory;

        const COMMAND_NAME: &'static str = "reserve-inventory";
        const SCHEMA_VERSION: u32 = 1;
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
                AggregateId::new(&event.order_id).expect("valid order ID"),
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

    struct RejectInventoryWhenOrderPlaced;

    impl IntegrationEventHandler<OrderPlaced> for RejectInventoryWhenOrderPlaced {
        type Error = Infallible;

        fn handle(
            &self,
            event: &OrderPlaced,
            commands: &mut CommandContext,
        ) -> Result<(), Self::Error> {
            commands.issue(
                AggregateId::new(&event.order_id).expect("valid order ID"),
                ReserveInventory {
                    order_id: event.order_id.clone(),
                    quantity: 0,
                },
            );
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
                    AggregateId::new(&event.order_id).expect("valid order ID"),
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
                AggregateId::new(&event.order_id).expect("valid order ID"),
                ReserveInventory {
                    order_id: event.order_id.clone(),
                    quantity: event.quantity,
                },
            );
            Err("invalid mapping")
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
            MessageTimestamp::from_unix_milliseconds(1).unwrap(),
            OrderPlaced {
                order_id: "order-42".to_owned(),
                quantity: 3,
            },
        )
        .unwrap()
    }

    fn durable() -> DurableName {
        DurableName::new("shop", "inventory", "reserve-orders", 1).unwrap()
    }

    fn stream() -> StreamId {
        StreamId::new(
            AggregateType::new("inventory").unwrap(),
            AggregateId::new("order-42").unwrap(),
        )
    }

    #[test]
    fn handler_tests_can_inspect_an_issued_command_without_a_target_wrapper() {
        let mut commands = CommandContext::new();
        ReserveInventoryWhenOrderPlaced
            .handle(envelope().payload(), &mut commands)
            .unwrap();

        let (aggregate_id, command) = commands.issued::<ReserveInventory>().unwrap();
        assert_eq!(aggregate_id.as_str(), "order-42");
        assert_eq!(command.quantity, 3);
    }

    #[tokio::test]
    async fn redelivery_is_an_exact_replay_with_propagated_context() {
        let store = rostfrei_core::InMemoryEventStore::new();
        let processor = IntegrationEventProcessor::new(
            store.clone(),
            durable(),
            ReserveInventoryWhenOrderPlaced,
        );
        let envelope = envelope();

        let first = processor.process(&envelope).await.unwrap();
        let second = processor.process(&envelope).await.unwrap();

        let first = first.receipt().unwrap();
        let second = second.receipt().unwrap();
        assert!(!first.is_exact_replay());
        assert!(second.is_exact_replay());
        assert_eq!(first.events()[0].event_id(), second.events()[0].event_id());
        assert_eq!(
            first.events()[0].correlation_id().unwrap().as_str(),
            "checkout-42"
        );
        assert_eq!(
            first.events()[0].causation_id().unwrap().as_str(),
            "order-placed-42"
        );
    }

    #[tokio::test]
    async fn zero_commands_is_a_successful_no_op() {
        let processor = IntegrationEventProcessor::new(
            rostfrei_core::InMemoryEventStore::new(),
            durable(),
            IgnoreOrderPlaced,
        );

        assert!(matches!(
            processor.process(&envelope()).await.unwrap(),
            IntegrationEventOutcome::NoCommand
        ));
    }

    #[tokio::test]
    async fn command_rejection_is_a_typed_business_outcome() {
        let processor = IntegrationEventProcessor::new(
            rostfrei_core::InMemoryEventStore::new(),
            durable(),
            RejectInventoryWhenOrderPlaced,
        );

        let outcome = processor.process(&envelope()).await.unwrap();
        let rejection = outcome.rejection().unwrap();
        assert_eq!(rejection.command_name(), "reserve-inventory");
        assert_eq!(
            rejection.value::<InventoryUnavailable>(),
            Some(&InventoryUnavailable {
                order_id: "order-42".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn multiple_commands_fail_before_execution() {
        let store = rostfrei_core::InMemoryEventStore::new();
        let processor = IntegrationEventProcessor::new(store.clone(), durable(), IssueTwice);

        assert!(matches!(
            processor.process(&envelope()).await,
            Err(IntegrationEventProcessingError::MultipleCommands)
        ));
        assert!(store.load(&stream()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn handler_failure_discards_its_issued_command() {
        let store = rostfrei_core::InMemoryEventStore::new();
        let processor = IntegrationEventProcessor::new(store.clone(), durable(), FailAfterIssue);

        assert!(matches!(
            processor.process(&envelope()).await,
            Err(IntegrationEventProcessingError::Handler("invalid mapping"))
        ));
        assert!(store.load(&stream()).await.unwrap().is_empty());
    }
}
