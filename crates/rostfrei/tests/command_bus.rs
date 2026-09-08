#![allow(clippy::panic_in_result_fn)]

use std::{convert::Infallible, error::Error, sync::Arc};

use async_trait::async_trait;
use rostfrei::{
    Aggregate, Apply, Command, CommandBindingRegistrationError, CommandBus, CommandBusErrorKind,
    CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult,
    CommandMessageAdapter, CommandProcessor, CommandProcessorErrorKind, CommandRequest,
    DomainEvent, DomainIdentity, DynamicCommandRequest, EncodedCommand, Entity, EventStore,
    InMemoryEventStore, InMemoryMessagingAdapter, Initialize, OperationId, StreamAggregateId,
    StreamId, command_execution_fingerprint, command_message_id,
};
use rostfrei::{BoundedContext, InfallibleCommandRejectionMapper};
use rostfrei_messaging_core::{
    ApplicationName, CommandResponseOutcome, CorrelationId, MessageId, MessageTimestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(BoundedContext)]
#[domain(id = "ledger", label = "Ledger")]
struct Ledger;

#[derive(BoundedContext)]
#[domain(id = "other-ledger", label = "Other ledger")]
struct OtherLedger;

#[derive(DomainIdentity)]
#[allow(dead_code)]
struct AccountId(String);

#[derive(Entity)]
#[domain(id = "account", label = "Account")]
#[allow(dead_code)]
struct Account {
    id: AccountId,
    balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, DomainEvent)]
#[domain(id = "account-credited", label = "Account credited")]
struct AccountCredited {
    amount: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, DomainEvent)]
#[domain(id = "balance-observed", label = "Balance observed")]
struct BalanceObserved {
    balance: i64,
}

#[derive(rostfrei::AggregateEvents)]
enum AccountEvents {
    AccountCredited(AccountCredited),
    BalanceObserved(BalanceObserved),
}

#[derive(Aggregate)]
#[domain(id = "account", label = "Account")]
struct AccountAggregate;

impl rostfrei::AggregateDefinition for AccountAggregate {
    type Context = Ledger;
    type Root = Account;
    type Event = AccountEvents;
}

impl Initialize<AccountAggregate> for Account {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: AccountId(stream_id.aggregate_id().as_str().to_owned()),
            balance: 0,
        }
    }
}

impl Apply<AccountCredited> for Account {
    fn apply(&mut self, event: &AccountCredited) {
        self.balance = self.balance.saturating_add(event.amount);
    }
}

impl Apply<BalanceObserved> for Account {
    fn apply(&mut self, _event: &BalanceObserved) {}
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(context = Ledger, id = "credit-account", label = "Credit account")]
struct CreditAccount {
    account_id: String,
    amount: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(context = Ledger, id = "observe-balance", label = "Observe balance")]
struct ObserveBalance {
    account_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(context = OtherLedger, id = "foreign-command", label = "Foreign command")]
struct ForeignCommand;

struct ForeignCommandHandler;

#[async_trait]
impl CommandHandler<ForeignCommand> for ForeignCommandHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        _command: &ForeignCommand,
        _execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        Ok(CommandDecision::Accepted)
    }
}

struct CreditAccountHandler;

#[async_trait]
impl CommandHandler<CreditAccount> for CreditAccountHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &CreditAccount,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut account = execution
            .load::<AccountAggregate>(&command.account_id)
            .await?;
        account.aggregate_mut().raise(AccountCredited {
            amount: command.amount,
        });
        Ok(CommandDecision::Accepted)
    }
}

struct ObserveBalanceHandler;

#[async_trait]
impl CommandHandler<ObserveBalance> for ObserveBalanceHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &ObserveBalance,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut account = execution
            .load::<AccountAggregate>(&command.account_id)
            .await?;
        let balance = account.aggregate().state().balance;
        account.aggregate_mut().raise(BalanceObserved { balance });
        Ok(CommandDecision::Accepted)
    }
}

fn context() -> TestResult<rostfrei_messaging_core::BoundedContext> {
    Ok(ApplicationName::new("command-bus-test")?.bounded_context("ledger")?)
}

#[test]
fn processor_rejects_commands_registered_for_another_context() -> TestResult {
    let store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let mut processor = CommandProcessor::new(context()?.name().clone(), store);

    let result = processor.register::<ForeignCommand, ForeignCommandHandler>(
        ForeignCommandHandler,
        InfallibleCommandRejectionMapper,
    );
    let Err(error) = result else {
        panic!("foreign command registration must fail");
    };

    assert!(matches!(
        error,
        CommandBindingRegistrationError::ContextMismatch {
            command_context: "other-ledger",
            ..
        }
    ));
    Ok(())
}

fn registered_processor(store: InMemoryEventStore) -> TestResult<CommandProcessor> {
    let store: Arc<dyn EventStore> = Arc::new(store);
    let mut processor = CommandProcessor::new(context()?.name().clone(), store);
    processor.register::<CreditAccount, CreditAccountHandler>(
        CreditAccountHandler,
        InfallibleCommandRejectionMapper,
    )?;
    processor.register::<ObserveBalance, ObserveBalanceHandler>(
        ObserveBalanceHandler,
        InfallibleCommandRejectionMapper,
    )?;
    Ok(processor)
}

fn request<C>(operation: &str, command: C) -> TestResult<CommandRequest<C>> {
    Ok(CommandRequest::new(OperationId::new(operation)?, command))
}

#[tokio::test]
async fn processor_rejects_incoming_commands_for_another_context() -> TestResult {
    let processor = Arc::new(registered_processor(InMemoryEventStore::new())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::clone(&processor)));
    let erased: Arc<dyn CommandMessageAdapter> = adapter;
    let foreign_context =
        ApplicationName::new("command-bus-test")?.bounded_context("other-ledger")?;
    let bus = CommandBus::new(foreign_context, erased);
    let encoded = bus.encode_dynamic(DynamicCommandRequest::new(
        OperationId::new("foreign-command")?,
        "foreign-command",
        1,
        json!({}),
    )?)?;

    let error = processor.process(&encoded).await.unwrap_err();

    assert_eq!(error.kind(), CommandProcessorErrorKind::InvalidMessage);
    assert!(error.message().contains("does not match processor context"));
    Ok(())
}

#[tokio::test]
async fn registered_command_types_dispatch_without_command_name_branching() -> TestResult {
    let store = InMemoryEventStore::new();
    let processor = Arc::new(registered_processor(store.clone())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(processor));
    let erased: Arc<dyn CommandMessageAdapter> = adapter.clone();
    let bus = CommandBus::new(context()?, erased);

    let credit = bus
        .dispatch::<CreditAccount>(request(
            "credit-1",
            CreditAccount {
                account_id: "account-1".to_owned(),
                amount: 7,
            },
        )?)
        .await?;
    let observed = bus
        .dispatch::<ObserveBalance>(request(
            "observe-1",
            ObserveBalance {
                account_id: "account-1".to_owned(),
            },
        )?)
        .await?;
    assert!(matches!(
        credit.response().outcome(),
        CommandResponseOutcome::Accepted
    ));
    assert!(matches!(
        observed.response().outcome(),
        CommandResponseOutcome::Accepted
    ));

    let history = store
        .load(&StreamId::new(
            rostfrei::StreamAggregateType::new(AccountAggregate::aggregate_type().into_owned())?,
            StreamAggregateId::new("account-1")?,
        ))
        .await?;
    assert_eq!(history.len(), 2);
    assert_eq!(
        history.first().map(rostfrei::RecordedEvent::event_type),
        Some("account-credited")
    );
    assert_eq!(
        history.get(1).map(rostfrei::RecordedEvent::event_type),
        Some("balance-observed")
    );
    assert_eq!(
        history.get(1).map(rostfrei::RecordedEvent::payload),
        Some(br#"{"balance":7}"#.as_slice())
    );
    Ok(())
}

#[test]
fn encoding_is_canonical_and_identity_is_stable() -> TestResult {
    let processor = Arc::new(registered_processor(InMemoryEventStore::new())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(processor));
    let erased: Arc<dyn CommandMessageAdapter> = adapter;
    let bus = CommandBus::new(context()?, erased);
    let timestamp = MessageTimestamp::from_unix_milliseconds(1_000)?;
    let correlation = CorrelationId::new("canonical-correlation")?;

    let first = bus.encode::<CreditAccount>(
        request(
            "canonical-command",
            CreditAccount {
                account_id: "account-1".to_owned(),
                amount: 7,
            },
        )?
        .with_correlation_id(correlation.clone())
        .with_created_at(timestamp),
    )?;
    let second = bus.encode::<CreditAccount>(
        request(
            "canonical-command",
            CreditAccount {
                account_id: "account-1".to_owned(),
                amount: 7,
            },
        )?
        .with_correlation_id(correlation)
        .with_created_at(timestamp),
    )?;
    assert_eq!(first, second);
    let wire: serde_json::Value = serde_json::from_slice(first.payload())?;
    assert!(wire.pointer("/payload/events_caused_by_command").is_none());
    assert!(wire.pointer("/payload/aggregate_type").is_none());
    assert!(wire.pointer("/payload/aggregate_id").is_none());

    let left = command_execution_fingerprint(
        "ledger",
        "credit-account",
        1,
        &json!({ "z": 1, "a": { "y": 2, "b": 3 } }),
    )?;
    let right = command_execution_fingerprint(
        "ledger",
        "credit-account",
        1,
        &json!({ "a": { "b": 3, "y": 2 }, "z": 1 }),
    )?;
    assert_eq!(left, right);
    Ok(())
}

#[test]
fn command_provenance_participates_in_execution_and_message_identity() -> TestResult {
    let processor = Arc::new(registered_processor(InMemoryEventStore::new())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(processor));
    let erased: Arc<dyn CommandMessageAdapter> = adapter;
    let bus = CommandBus::new(context()?, erased);
    let ordinary = bus.encode::<CreditAccount>(request(
        "provenance-command",
        CreditAccount {
            account_id: "account-1".to_owned(),
            amount: 4,
        },
    )?)?;

    let mut integration_wire: serde_json::Value = serde_json::from_slice(ordinary.payload())?;
    let ordinary_payload = integration_wire
        .pointer("/payload/payload")
        .ok_or("command payload is missing")?;
    assert_eq!(
        ordinary.fingerprint(),
        command_execution_fingerprint("ledger", "credit-account", 1, ordinary_payload)?
    );
    let routed = integration_wire
        .get_mut("payload")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("command envelope payload is not an object")?;
    routed.insert("events_caused_by_command".to_owned(), json!(true));
    let integration = EncodedCommand::from_delivery(
        ordinary.address().clone(),
        ordinary.message_id().clone(),
        serde_json::to_vec(&integration_wire)?,
    )?;

    assert_ne!(ordinary.fingerprint(), integration.fingerprint());
    let integration_message_id = command_message_id(
        integration.address(),
        integration.operation_id(),
        integration.fingerprint(),
        integration.correlation_id(),
        None,
    )?;
    assert_ne!(ordinary.message_id(), &integration_message_id);
    Ok(())
}

#[tokio::test]
async fn processor_rejects_tampered_identity_and_bus_bounds_payloads() -> TestResult {
    let processor = Arc::new(registered_processor(InMemoryEventStore::new())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::clone(&processor)));
    let erased: Arc<dyn CommandMessageAdapter> = adapter;
    let bus = CommandBus::new(context()?, erased);
    let encoded = bus.encode::<CreditAccount>(request(
        "tampered-command",
        CreditAccount {
            account_id: "account-1".to_owned(),
            amount: 4,
        },
    )?)?;
    let tampered = EncodedCommand::from_delivery(
        encoded.address().clone(),
        MessageId::new("different-message-id")?,
        encoded.payload().to_vec(),
    )?;
    let error = processor
        .process(&tampered)
        .await
        .expect_err("tampered command identity should fail");
    assert_eq!(error.kind(), CommandProcessorErrorKind::InvalidMessage);

    let oversized = "x".repeat(rostfrei_messaging_core::MAX_MESSAGE_PAYLOAD_BYTES);
    let error = bus
        .dispatch_dynamic(DynamicCommandRequest::new(
            OperationId::new("oversized-command")?,
            "unknown-command",
            1,
            json!({ "content": oversized }),
        )?)
        .await
        .expect_err("oversized command payload should fail");
    assert_eq!(error.kind(), CommandBusErrorKind::PayloadTooLarge);
    Ok(())
}
rostfrei::install_macro_support!();
