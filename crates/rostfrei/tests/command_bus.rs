#![allow(clippy::panic_in_result_fn)]

use std::{convert::Infallible, error::Error, sync::Arc};

use rostfrei::{
    Aggregate, AggregateInstance, Apply, CommandBus, CommandBusErrorKind, CommandHandler,
    CommandMessageAdapter, CommandProcessor, CommandProcessorErrorKind, CommandRequest,
    Command, DomainEvent, DomainIdentity, DynamicCommandRequest, EncodedCommand, Entity,
    EventStore, InMemoryEventStore, InMemoryMessagingAdapter, Initialize, OperationId,
    StreamAggregateId, StreamId, command_execution_fingerprint,
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

#[derive(DomainIdentity)]
#[domain(owner = Account)]
#[allow(dead_code)]
struct AccountId(String);

#[derive(Entity)]
#[domain(id = "account", label = "Account", owner = AccountAggregate)]
#[allow(dead_code)]
struct Account {
    #[domain(identity)]
    id: AccountId,
    balance: i64,
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

#[derive(Aggregate)]
#[domain(
    id = "account",
    label = "Account",
    context = Ledger,
    root = Account,
    events = [AccountCredited, BalanceObserved]
)]
struct AccountAggregate;

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
#[domain(
    id = "credit-account",
    label = "Credit account",
    owner = AccountAggregate,
    json,
    runtime
)]
struct CreditAccount {
    amount: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Command)]
#[domain(
    id = "observe-balance",
    label = "Observe balance",
    owner = AccountAggregate,
    json,
    runtime
)]
struct ObserveBalance;

impl CommandHandler<CreditAccount> for AccountAggregate {
    type Rejection = Infallible;

    fn handle(
        command: &CreditAccount,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.raise(AccountCredited {
            amount: command.amount,
        });
        Ok(())
    }
}

impl CommandHandler<ObserveBalance> for AccountAggregate {
    type Rejection = Infallible;

    fn handle(
        _command: &ObserveBalance,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.raise(BalanceObserved {
            balance: aggregate.state().balance,
        });
        Ok(())
    }
}

fn context() -> TestResult<rostfrei_messaging_core::BoundedContext> {
    Ok(ApplicationName::new("command-bus-test")?.bounded_context("ledger")?)
}

fn registered_processor(store: InMemoryEventStore) -> TestResult<CommandProcessor> {
    let store: Arc<dyn EventStore> = Arc::new(store);
    let mut processor = CommandProcessor::new(store);
    processor.register::<CreditAccount, _>(InfallibleCommandRejectionMapper)?;
    processor.register::<ObserveBalance, _>(InfallibleCommandRejectionMapper)?;
    Ok(processor)
}

fn request<C>(operation: &str, command: C) -> TestResult<CommandRequest<C>> {
    Ok(CommandRequest::new(
        OperationId::new(operation)?,
        StreamAggregateId::new("account-1")?,
        command,
    ))
}

#[tokio::test]
async fn registered_command_types_dispatch_without_command_name_branching() -> TestResult {
    let store = InMemoryEventStore::new();
    let processor = Arc::new(registered_processor(store.clone())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(processor));
    let erased: Arc<dyn CommandMessageAdapter> = adapter.clone();
    let bus = CommandBus::new(context()?, erased);

    let credit = bus
        .dispatch(request("credit-1", CreditAccount { amount: 7 })?)
        .await?;
    let observed = bus.dispatch(request("observe-1", ObserveBalance)?).await?;
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

    let first = bus.encode(
        request("canonical-command", CreditAccount { amount: 7 })?
            .with_correlation_id(correlation.clone())
            .with_created_at(timestamp),
    )?;
    let second = bus.encode(
        request("canonical-command", CreditAccount { amount: 7 })?
            .with_correlation_id(correlation)
            .with_created_at(timestamp),
    )?;
    assert_eq!(first, second);

    let left = command_execution_fingerprint(
        "ledger/account",
        "account-1",
        "credit-account",
        1,
        &json!({ "z": 1, "a": { "y": 2, "b": 3 } }),
    )?;
    let right = command_execution_fingerprint(
        "ledger/account",
        "account-1",
        "credit-account",
        1,
        &json!({ "a": { "b": 3, "y": 2 }, "z": 1 }),
    )?;
    assert_eq!(left, right);
    Ok(())
}

#[tokio::test]
async fn processor_rejects_tampered_identity_and_bus_bounds_payloads() -> TestResult {
    let processor = Arc::new(registered_processor(InMemoryEventStore::new())?);
    let adapter = Arc::new(InMemoryMessagingAdapter::new(Arc::clone(&processor)));
    let erased: Arc<dyn CommandMessageAdapter> = adapter;
    let bus = CommandBus::new(context()?, erased);
    let encoded = bus.encode(request("tampered-command", CreditAccount { amount: 4 })?)?;
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
            "ledger/account",
            StreamAggregateId::new("account-1")?,
            "unknown-command",
            1,
            json!({ "content": oversized }),
        )?)
        .await
        .expect_err("oversized command payload should fail");
    assert_eq!(error.kind(), CommandBusErrorKind::Encoding);
    Ok(())
}
