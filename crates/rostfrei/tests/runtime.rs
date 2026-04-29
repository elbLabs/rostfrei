use std::{
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use rostfrei::{
    Aggregate as RuntimeAggregate, AggregateInstance, Apply, CommandExecutionError, CommandHandler,
    CommandOutcome, CommittedDomainEvent, ContentFingerprint, DomainEventDispatchOutcome,
    DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventBatch, EventCodec, EventCodecError, EventCodecErrorKind,
    EventStore, EventVariant, ExecutionMetadata, Executor, ExpectedVersion, InMemoryEventStore,
    Initialize, NewEvent, OperationId, RecordedEvent, StreamAggregateId, StreamId,
};
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
    #[rostfrei(identity)]
    id: AccountId,
    balance: i64,
    observed_balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;
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

    #[rostfrei::domain_actions(aggregate(instance = AccountActions))]
    pub trait AccountActionContract {
        #[action(
            id = "deposit",
            label = "Deposit money",
            raises = [MoneyDeposited]
        )]
        fn deposit(&mut self, input: i64);

        #[action(
            id = "observe-balance",
            label = "Observe balance",
            raises = [BalanceObserved]
        )]
        fn observe_balance(&mut self);

        #[action(
            id = "deposit-and-observe",
            label = "Deposit money and observe balance",
            raises = [MoneyDeposited, BalanceObserved]
        )]
        fn deposit_and_observe(&mut self, input: i64);
    }

    impl AccountActions for AggregateInstance<AccountAggregate> {
        fn deposit(&mut self, input: i64) {
            self.raise(MoneyDeposited { amount: input });
        }

        fn observe_balance(&mut self) {
            self.raise(BalanceObserved {
                balance: self.state().balance,
            });
        }

        fn deposit_and_observe(&mut self, input: i64) {
            self.raise(MoneyDeposited { amount: input });

            self.raise(BalanceObserved {
                balance: self.state().balance,
            });
        }
    }
}

use account_actions::AccountActions as _;

struct DepositAndObserve {
    account_id: &'static str,
    amount: i64,
}

impl CommandHandler<DepositAndObserve> for AccountAggregate {
    type Rejection = &'static str;

    fn handle(
        command: &DepositAndObserve,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        if aggregate.state().id.0 != command.account_id {
            return Err("stream identity was not used to initialize the aggregate");
        }
        aggregate.deposit(command.amount);
        aggregate.observe_balance();
        Ok(())
    }
}

struct DepositThenReject {
    amount: i64,
}

impl CommandHandler<DepositThenReject> for AccountAggregate {
    type Rejection = &'static str;

    fn handle(
        command: &DepositThenReject,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.deposit(command.amount);
        Err("deliberate rejection")
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

struct TextEventCodec;

impl EventCodec<AccountAggregate> for TextEventCodec {
    fn encode(
        &self,
        event: &<AccountAggregate as RuntimeAggregate>::Event,
        event_id: rostfrei::EventId,
    ) -> Result<NewEvent, EventCodecError> {
        let (event_type, schema_version, payload) =
            if let Some(event) = EventVariant::<MoneyDeposited>::event(event) {
                (
                    "money-deposited",
                    2,
                    format!("deposit:{}", event.amount).into_bytes(),
                )
            } else if let Some(event) = EventVariant::<BalanceObserved>::event(event) {
                (
                    "balance-observed",
                    1,
                    format!("observed:{}", event.balance).into_bytes(),
                )
            } else {
                return Err(EventCodecError::new(
                    EventCodecErrorKind::UnknownEventType,
                    "unknown generated event variant",
                ));
            };
        NewEvent::new(event_id, event_type, schema_version, payload).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, error.to_string())
        })
    }

    fn decode(
        &self,
        event: &RecordedEvent,
    ) -> Result<<AccountAggregate as RuntimeAggregate>::Event, EventCodecError> {
        let payload = std::str::from_utf8(event.payload()).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })?;
        let value = payload
            .split_once(':')
            .and_then(|(_, value)| value.parse::<i64>().ok())
            .ok_or_else(|| {
                EventCodecError::new(EventCodecErrorKind::MalformedPayload, "invalid text event")
            })?;
        match (event.event_type(), event.schema_version()) {
            ("money-deposited", 2) => Ok(MoneyDeposited { amount: value }.into()),
            ("balance-observed", 1) => Ok(BalanceObserved { balance: value }.into()),
            ("money-deposited" | "balance-observed", _) => Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported text event version",
            )),
            _ => Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                "unknown text event type",
            )),
        }
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

fn metadata(stream_id: &StreamId, operation: &str) -> TestResult<ExecutionMetadata> {
    let operation_id = OperationId::new(operation)
        .map_err(|error| fixture_error("account operation ID", error))?;
    Ok(ExecutionMetadata::new(
        stream_id.clone(),
        operation_id,
        ContentFingerprint::digest(operation),
    ))
}

#[tokio::test]
async fn command_composes_generated_actions_and_replays_their_events() {
    let stream = stream("account-1").expect("valid account stream fixture");
    let executor = Executor::new(InMemoryEventStore::new());

    let first = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "deposit-1").expect("valid first deposit metadata fixture"),
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
        .execute::<AccountAggregate, _>(
            metadata(&stream, "deposit-2").expect("valid second deposit metadata fixture"),
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
fn executable_action_can_raise_multiple_declared_event_types() {
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
    let executor = Executor::new(store.clone());

    let outcome = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "rejected-deposit").expect("valid rejected deposit metadata fixture"),
            &DepositThenReject { amount: 9 },
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
fn executable_actions_model_every_event_type_they_may_raise() {
    let actions = <AccountAggregate as account_actions::AccountActionContract>::
        __DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE;

    assert_eq!(actions.len(), 3);
    assert_eq!(
        actions[0].raises,
        &[<MoneyDeposited as rostfrei::DomainEventType<AccountAggregate>>::DESCRIPTOR.id]
    );
    assert_eq!(
        actions[1].raises,
        &[<BalanceObserved as rostfrei::DomainEventType<AccountAggregate>>::DESCRIPTOR.id]
    );
    assert_eq!(
        actions[2].raises,
        &[
            <MoneyDeposited as rostfrei::DomainEventType<AccountAggregate>>::DESCRIPTOR.id,
            <BalanceObserved as rostfrei::DomainEventType<AccountAggregate>>::DESCRIPTOR.id,
        ]
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

#[tokio::test]
async fn custom_codec_remains_an_explicit_override_for_the_authored_event_enum() {
    let stream = stream("custom-account").expect("valid custom codec stream fixture");
    let executor = Executor::with_codec(InMemoryEventStore::new(), TextEventCodec);
    let outcome = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "custom-deposit")
                .expect("valid custom codec execution metadata fixture"),
            &DepositAndObserve {
                account_id: "custom-account",
                amount: 11,
            },
        )
        .await
        .expect("custom codec execution");
    let CommandOutcome::Accepted(outcome) = outcome else {
        panic!("deposit should be accepted");
    };

    assert_eq!(outcome.events()[0].payload(), b"deposit:11");
    assert_eq!(outcome.events()[1].payload(), b"observed:11");
}

#[test]
fn domain_model_projects_event_set_once_in_enum_declaration_order() {
    let model = rostfrei::domain_model! {
        contexts: [Banking],
        aggregates: [AccountAggregate],
        entities: [Account],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
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
    let seed = metadata(&stream, "seed-invalid-history")?;
    let event = NewEvent::new(
        seed.event_id(0),
        event_type,
        schema_version,
        payload.to_vec(),
    )
    .map_err(|error| fixture_error("raw replay event", error))?;
    let batch = EventBatch::new(
        seed.commit_id().clone(),
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

    let result = Executor::new(store)
        .execute::<AccountAggregate, _>(
            metadata(&stream, "after-invalid-history")?,
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
