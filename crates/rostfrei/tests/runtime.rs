use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rostfrei::{
    Aggregate as RuntimeAggregate, AggregateInstance, Apply, CommandExecutionError, CommandHandler,
    CommandOutcome, CommittedDomainEvent, ContentFingerprint, DomainEventDispatchOutcome,
    DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError, EventBatch, EventCodec,
    EventCodecError, EventCodecErrorKind, EventStore, EventVariant, ExecutionMetadata, Executor,
    ExpectedVersion, InMemoryEventStore, Initialize, NewEvent, OperationId, RecordedEvent,
    StreamAggregateId, StreamId,
};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "banking", label = "Banking")]
struct Banking;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = Account)]
struct AccountId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "account", label = "Account", owner = AccountAggregate)]
struct Account {
    #[rostfrei(identity)]
    id: AccountId,
    balance: i64,
    observed_balance: i64,
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

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "account",
    label = "Account",
    context = Banking,
    root = Account,
    actions = [account_actions::AccountActionContract],
    events = [MoneyDeposited, BalanceObserved]
)]
struct AccountAggregate;

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
        self.balance += event.amount;
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
            .expect("deposit handler lock")
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

fn stream(id: &str) -> StreamId {
    StreamId::new(
        rostfrei::StreamAggregateType::new(
            <AccountAggregate as RuntimeAggregate>::aggregate_type(),
        )
        .expect("valid compiled aggregate type"),
        StreamAggregateId::new(id).expect("valid aggregate id"),
    )
}

fn metadata(stream_id: &StreamId, operation: &str) -> ExecutionMetadata {
    ExecutionMetadata::new(
        stream_id.clone(),
        OperationId::new(operation).expect("valid operation id"),
        ContentFingerprint::digest(operation),
    )
}

#[tokio::test]
async fn command_composes_generated_actions_and_replays_their_events() {
    let stream = stream("account-1");
    let executor = Executor::new(InMemoryEventStore::new());

    let first = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "deposit-1"),
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
            metadata(&stream, "deposit-2"),
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
    let mut aggregate = AggregateInstance::<AccountAggregate>::new(stream("multi-event-action"));

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
    let stream = stream("rejected-account");
    let store = InMemoryEventStore::new();
    let executor = Executor::new(store.clone());

    let outcome = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "rejected-deposit"),
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
    let actions = <AccountAggregate as rostfrei::AggregateType>::ACTION_CONTRACTS[0];

    assert_eq!(actions.len(), 3);
    assert!(actions.iter().all(|action| action.output.is_none()));
    assert_eq!(
        actions[0].raises,
        &[<MoneyDeposited as rostfrei::DomainEventType>::DESCRIPTOR.id]
    );
    assert_eq!(
        actions[1].raises,
        &[<BalanceObserved as rostfrei::DomainEventType>::DESCRIPTOR.id]
    );
    assert_eq!(
        actions[2].raises,
        &[
            <MoneyDeposited as rostfrei::DomainEventType>::DESCRIPTOR.id,
            <BalanceObserved as rostfrei::DomainEventType>::DESCRIPTOR.id,
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
        replay_error("unknown-event", 1, b"{}").await,
        EventCodecErrorKind::UnknownEventType
    );
    assert_eq!(
        replay_error("money-deposited", 1, br#"{"amount":4}"#).await,
        EventCodecErrorKind::UnsupportedSchemaVersion
    );
    assert_eq!(
        replay_error("money-deposited", 2, b"not-json").await,
        EventCodecErrorKind::MalformedPayload
    );
}

#[tokio::test]
async fn custom_codec_remains_an_explicit_override_without_naming_the_generated_enum() {
    let stream = stream("custom-account");
    let executor = Executor::with_codec(InMemoryEventStore::new(), TextEventCodec);
    let outcome = executor
        .execute::<AccountAggregate, _>(
            metadata(&stream, "custom-deposit"),
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
fn domain_model_projects_attached_events_once_in_aggregate_declaration_order() {
    let model = rostfrei::domain_model! {
        contexts: [Banking],
        aggregates: [AccountAggregate],
        entities: [Account],
        identities: [AccountId],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    };
    let events = model["domainEvents"].as_array().expect("domain events");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"]["local"], "money-deposited");
    assert_eq!(events[0]["schemaVersion"], 2);
    assert_eq!(events[1]["id"]["local"], "balance-observed");
}

async fn replay_error(
    event_type: &str,
    schema_version: u32,
    payload: &[u8],
) -> EventCodecErrorKind {
    let stream = stream("invalid-history");
    let store = InMemoryEventStore::new();
    let seed = metadata(&stream, "seed-invalid-history");
    let event = NewEvent::new(
        seed.event_id(0),
        event_type,
        schema_version,
        payload.to_vec(),
    )
    .expect("valid raw event envelope");
    let batch = EventBatch::new(
        seed.commit_id().clone(),
        seed.operation_id().clone(),
        seed.operation_fingerprint(),
        vec![event],
    )
    .expect("non-empty batch");
    store
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await
        .expect("seed invalid history");

    let error = Executor::new(store)
        .execute::<AccountAggregate, _>(
            metadata(&stream, "after-invalid-history"),
            &DepositAndObserve {
                account_id: "invalid-history",
                amount: 1,
            },
        )
        .await
        .expect_err("invalid history must fail closed");
    match error {
        CommandExecutionError::Codec(error) => error.kind(),
        CommandExecutionError::Store(error) => {
            panic!("expected codec error, got store error: {error:?}")
        }
    }
}
