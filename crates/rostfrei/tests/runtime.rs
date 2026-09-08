use std::{
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use rostfrei::{
    Aggregate as RuntimeAggregate, AggregateInstance, Apply, CommandDecision, CommandExecution,
    CommandExecutionError, CommandExecutionMetadata, CommandExecutor, CommandHandler,
    CommandHandlingResult, CommandOutcome, CommittedDomainEvent, ContentFingerprint,
    DomainEventDispatchOutcome, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventBatch, EventCodecErrorKind, EventStore, EventVariant,
    ExpectedVersion, InMemoryEventStore, Initialize, NewEvent, OperationId, StreamAggregateId,
    StreamId,
};
use rostfrei_core::{derive_commit_id, derive_event_id};
use serde::{Deserialize, Serialize};

type TestResult<T = ()> = Result<T, TestError>;

#[derive(Debug)]
enum TestError {
    InvalidFixture {
        context: &'static str,
        message: String,
    },
    UnexpectedFailure {
        context: &'static str,
        message: String,
    },
    ExpectedFailure {
        context: &'static str,
    },
}

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFixture { context, message } => {
                write!(formatter, "{context}: invalid test fixture: {message}")
            }
            Self::UnexpectedFailure { context, message } => {
                write!(formatter, "{context}: unexpected failure: {message}")
            }
            Self::ExpectedFailure { context } => {
                write!(formatter, "{context}: expected the operation to fail")
            }
        }
    }
}

impl std::error::Error for TestError {}

fn fixture_error(context: &'static str, error: impl fmt::Display) -> TestError {
    TestError::InvalidFixture {
        context,
        message: error.to_string(),
    }
}

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "banking", label = "Banking")]
struct Banking;

#[derive(rostfrei::DomainIdentity)]
struct AccountId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "account", label = "Account")]
struct Account {
    id: AccountId,
    balance: i64,
    observed_balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "money-deposited", label = "Money deposited", schema_version = 2)]
struct MoneyDeposited {
    amount: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "balance-observed", label = "Balance observed")]
struct BalanceObserved {
    balance: i64,
}

#[derive(rostfrei::AggregateEvents)]
enum AccountEvents {
    MoneyDeposited(MoneyDeposited),
    BalanceObserved(BalanceObserved),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "account", label = "Account")]
struct AccountAggregate;

impl rostfrei::AggregateDefinition for AccountAggregate {
    type Context = Banking;
    type Root = Account;
    type Event = AccountEvents;
}

impl Initialize<AccountAggregate> for Account {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: AccountId(stream_id.aggregate_id().as_str().to_owned()),
            balance: 0,
            observed_balance: 0,
        }
    }
}

impl Apply<MoneyDeposited> for Account {
    fn apply(&mut self, event: &MoneyDeposited) {
        self.balance = self.balance.wrapping_add(event.amount);
    }
}

impl Apply<BalanceObserved> for Account {
    fn apply(&mut self, event: &BalanceObserved) {
        self.observed_balance = event.balance;
    }
}

mod account_actions {
    use super::{AccountAggregate, AggregateInstance, BalanceObserved, MoneyDeposited};

    #[rostfrei::domain_action(id = "deposit", label = "Deposit money")]
    pub trait DepositAction {
        fn deposit(&mut self, input: i64);
    }

    impl DepositAction for AggregateInstance<AccountAggregate> {
        fn deposit(&mut self, input: i64) {
            self.raise(MoneyDeposited { amount: input });
        }
    }

    #[rostfrei::domain_action(id = "observe-balance", label = "Observe balance")]
    pub trait ObserveBalanceAction {
        fn observe_balance(&mut self);
    }

    impl ObserveBalanceAction for AggregateInstance<AccountAggregate> {
        fn observe_balance(&mut self) {
            self.raise(BalanceObserved {
                balance: self.state().balance,
            });
        }
    }

    #[rostfrei::domain_action(
        id = "deposit-and-observe",
        label = "Deposit money and observe balance"
    )]
    pub trait DepositAndObserveAction {
        fn deposit_and_observe(&mut self, input: i64);
    }

    impl DepositAndObserveAction for AggregateInstance<AccountAggregate> {
        fn deposit_and_observe(&mut self, input: i64) {
            self.raise(MoneyDeposited { amount: input });
            self.raise(BalanceObserved {
                balance: self.state().balance,
            });
        }
    }
}

use account_actions::{
    DepositAction as _, DepositAndObserveAction as _, ObserveBalanceAction as _,
};

struct DepositAndObserve {
    account_id: &'static str,
    amount: i64,
}

struct AccountCommandHandler;

#[async_trait]
impl CommandHandler<DepositAndObserve> for AccountCommandHandler {
    type Rejection = &'static str;

    async fn handle(
        &self,
        command: &DepositAndObserve,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut aggregate = execution
            .load::<AccountAggregate>(command.account_id)
            .await?;
        if aggregate.aggregate().state().id.0 != command.account_id {
            return Ok(CommandDecision::Rejected(
                "stream identity was not used to initialize the aggregate",
            ));
        }
        aggregate.aggregate_mut().deposit(command.amount);
        aggregate.aggregate_mut().observe_balance();
        Ok(CommandDecision::Accepted)
    }
}

struct DepositThenReject {
    account_id: &'static str,
    amount: i64,
}

#[async_trait]
impl CommandHandler<DepositThenReject> for AccountCommandHandler {
    type Rejection = &'static str;

    async fn handle(
        &self,
        command: &DepositThenReject,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut aggregate = execution
            .load::<AccountAggregate>(command.account_id)
            .await?;
        aggregate.aggregate_mut().deposit(command.amount);
        Ok(CommandDecision::Rejected("deliberate rejection"))
    }
}

#[derive(Default)]
struct DepositHandler {
    events: Mutex<Vec<MoneyDeposited>>,
}

#[async_trait]
impl DomainEventHandler<MoneyDeposited> for DepositHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, MoneyDeposited>,
    ) -> Result<(), DomainEventHandlerError> {
        self.events
            .lock()
            .map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    format!("deposit handler lock was poisoned: {error}"),
                )
            })?
            .push(event.event().clone());
        Ok(())
    }
}

fn stream(id: &str) -> TestResult<StreamId> {
    let aggregate_type = rostfrei::StreamAggregateType::new(
        <AccountAggregate as RuntimeAggregate>::aggregate_type(),
    )
    .map_err(|error| fixture_error("compiled account aggregate type", error))?;
    let aggregate_id =
        StreamAggregateId::new(id).map_err(|error| fixture_error("account aggregate ID", error))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

fn metadata(operation: &str) -> TestResult<CommandExecutionMetadata> {
    let operation_id = OperationId::new(operation)
        .map_err(|error| fixture_error("account operation ID", error))?;
    Ok(
        CommandExecutionMetadata::new(operation_id, ContentFingerprint::digest(operation))
            .with_bounded_context(
                rostfrei::BoundedContextName::new("banking")
                    .map_err(|error| fixture_error("banking context", error))?,
            ),
    )
}

#[tokio::test]
async fn registered_events_execute_and_replay_through_the_authored_event_set() {
    let executor = CommandExecutor::new(InMemoryEventStore::new());

    let first = executor
        .execute(
            &AccountCommandHandler,
            metadata("deposit-1").expect("valid first deposit metadata fixture"),
            &DepositAndObserve {
                account_id: "account-1",
                amount: 7,
            },
        )
        .await
        .expect("default JSON execution");
    let CommandOutcome::Accepted(first) = first else {
        panic!("deposit should be accepted");
    };
    assert_eq!(first.events().len(), 2);
    assert_eq!(first.events()[0].event_type(), "money-deposited");
    assert_eq!(first.events()[0].schema_version(), 2);
    assert_eq!(first.events()[0].payload(), br#"{"amount":7}"#);
    assert_eq!(first.events()[1].event_type(), "balance-observed");
    assert_eq!(first.events()[1].payload(), br#"{"balance":7}"#);

    let second = executor
        .execute(
            &AccountCommandHandler,
            metadata("deposit-2").expect("valid second deposit metadata fixture"),
            &DepositAndObserve {
                account_id: "account-1",
                amount: 3,
            },
        )
        .await
        .expect("all registered event types replay");
    let CommandOutcome::Accepted(second) = second else {
        panic!("deposit should be accepted");
    };
    assert_eq!(second.events()[1].payload(), br#"{"balance":10}"#);

    let handler = Arc::new(DepositHandler::default());
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher
        .register::<AccountAggregate, MoneyDeposited, _>("money-deposited", handler.clone())
        .expect("default committed-event codec registration");
    assert_eq!(
        dispatcher
            .dispatch(&first.events()[0])
            .await
            .expect("concrete committed event dispatch"),
        DomainEventDispatchOutcome::Handled
    );
    assert_eq!(
        handler
            .events
            .lock()
            .expect("deposit handler lock")
            .as_slice(),
        &[MoneyDeposited { amount: 7 }]
    );
}

#[test]
fn executable_action_can_raise_multiple_registered_event_types() {
    let mut aggregate = AggregateInstance::<AccountAggregate>::new(
        stream("multi-event-action").expect("valid multi-event action stream fixture"),
    );

    aggregate.deposit_and_observe(4);

    assert_eq!(aggregate.uncommitted_events().len(), 2);
    assert_eq!(
        EventVariant::<MoneyDeposited>::event(&aggregate.uncommitted_events()[0]),
        Some(&MoneyDeposited { amount: 4 })
    );
    assert_eq!(
        EventVariant::<BalanceObserved>::event(&aggregate.uncommitted_events()[1]),
        Some(&BalanceObserved { balance: 4 })
    );
    assert_eq!(aggregate.state().observed_balance, 4);
}

#[tokio::test]
async fn command_rejection_discards_events_raised_by_an_action() {
    let stream = stream("rejected-account").expect("valid rejected account stream fixture");
    let store = InMemoryEventStore::new();
    let executor = CommandExecutor::new(store.clone());

    let outcome = executor
        .execute(
            &AccountCommandHandler,
            metadata("rejected-deposit").expect("valid rejected deposit metadata fixture"),
            &DepositThenReject {
                account_id: "rejected-account",
                amount: 9,
            },
        )
        .await
        .expect("domain rejection");

    assert!(matches!(
        outcome,
        CommandOutcome::Rejected("deliberate rejection")
    ));
    assert!(
        store
            .load(&stream)
            .await
            .expect("load rejected stream")
            .is_empty()
    );
}

#[test]
fn compiled_aggregate_stream_type_includes_its_bounded_context() {
    assert_eq!(
        <AccountAggregate as RuntimeAggregate>::aggregate_type().as_ref(),
        "banking/account"
    );
}

#[tokio::test]
async fn generated_json_replay_fails_closed() {
    assert_eq!(
        replay_error("unknown-event", 1, b"{}")
            .await
            .expect("unknown event replay reaches codec classification"),
        EventCodecErrorKind::UnknownEventType
    );
    assert_eq!(
        replay_error("money-deposited", 1, br#"{"amount":4}"#)
            .await
            .expect("unsupported schema replay reaches codec classification"),
        EventCodecErrorKind::UnsupportedSchemaVersion
    );
    assert_eq!(
        replay_error("money-deposited", 2, b"not-json")
            .await
            .expect("malformed payload replay reaches codec classification"),
        EventCodecErrorKind::MalformedPayload
    );
}

#[test]
fn domain_model_projects_event_set_once_in_enum_declaration_order() {
    let model = rostfrei::domain_model! {
        contexts: [Banking],
        aggregates: [AccountAggregate],
        entities: [Account],
        value_objects: [],
        services: [],
        errors: [],
    }
    .expect("runtime test domain model projection");
    let events = model["domainEvents"].as_array().expect("domain events");

    let identities = model["domainIdentities"]
        .as_array()
        .expect("domain identities");
    assert_eq!(identities.len(), 1);
    assert!(identities[0].get("scalar").is_none());
    assert_eq!(identities[0]["id"]["owner"]["local"], "account");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"]["local"], "money-deposited");
    assert_eq!(events[0]["schemaVersion"], 2);
    assert_eq!(events[1]["id"]["local"], "balance-observed");
}

async fn replay_error(
    event_type: &str,
    schema_version: u32,
    payload: &[u8],
) -> TestResult<EventCodecErrorKind> {
    let stream = stream("invalid-history")?;
    let store = InMemoryEventStore::new();
    let seed = metadata("seed-invalid-history")?;
    let commit_id = derive_commit_id(&stream, seed.operation_id());
    let event = NewEvent::new(
        derive_event_id(&commit_id, 0),
        event_type,
        schema_version,
        payload.to_vec(),
    )
    .map_err(|error| fixture_error("raw replay event", error))?;
    let batch = EventBatch::new(
        commit_id,
        seed.operation_id().clone(),
        seed.operation_fingerprint(),
        vec![event],
    )
    .map_err(|error| fixture_error("raw replay batch", error))?;
    store
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await
        .map_err(|error| TestError::UnexpectedFailure {
            context: "seed invalid history",
            message: error.to_string(),
        })?;

    let result = CommandExecutor::new(store)
        .execute(
            &AccountCommandHandler,
            metadata("after-invalid-history")?,
            &DepositAndObserve {
                account_id: "invalid-history",
                amount: 1,
            },
        )
        .await;
    match result {
        Err(CommandExecutionError::Codec(error)) => Ok(error.kind()),
        Err(CommandExecutionError::Store(error)) => Err(TestError::UnexpectedFailure {
            context: "replay invalid history",
            message: error.to_string(),
        }),
        Ok(_) => Err(TestError::ExpectedFailure {
            context: "invalid history must fail closed",
        }),
    }
}
rostfrei::install_macro_support!();
