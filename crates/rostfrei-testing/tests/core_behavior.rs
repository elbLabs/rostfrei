use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, AppendOutcome, CommandHandler, CommittedDomainEvent,
    ContentFingerprint, DecisionContext, DomainEventDispatchOutcome, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, DomainEventRegistrationError,
    EnvelopeError, EventBatch, EventCodec, EventCodecError, EventCodecErrorKind, EventStore,
    EventStoreError, EventStoreErrorKind, ExecutionError, ExecutionMetadata, ExecutionOutcome,
    Executor, ExpectedVersion, InMemoryEventStore, NewEvent, OperationId, RecordedEvent, StreamId,
    StreamVersion,
};
use rostfrei_domain_runtime::{Apply, Initialize};
use rostfrei_messaging_core::{CausationId, CorrelationId};
use rostfrei_testing::{event_store_contract, given, DomainEventHandlerHarness};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImportProvenance {
    source_system: String,
    source_record: String,
    observed_at: String,
    import_batch: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AccountEvent {
    AccountStateImported {
        opening_balance: i64,
        provenance: ImportProvenance,
    },
    Credited {
        amount: i64,
    },
    BalanceObserved {
        balance: i64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Account {
    imported: bool,
    balance: i64,
    last_observed_balance: Option<i64>,
}

impl Aggregate for Account {
    type State = Self;
    type Event = AccountEvent;

    const AGGREGATE_TYPE: &'static str = "Account";

    fn initial(_stream_id: &StreamId) -> Self::State {
        Self {
            imported: false,
            balance: 0,
            last_observed_balance: None,
        }
    }

    fn apply(state: &mut Self::State, event: &Self::Event) {
        match event {
            AccountEvent::AccountStateImported {
                opening_balance, ..
            } => {
                state.imported = true;
                state.balance = *opening_balance;
            }
            AccountEvent::Credited { amount } => state.balance += amount,
            AccountEvent::BalanceObserved { balance } => {
                state.last_observed_balance = Some(*balance);
            }
        }
    }
}

enum AccountCommand {
    Import {
        opening_balance: i64,
        provenance: ImportProvenance,
    },
    CreditThenObserve {
        amount: i64,
    },
    RecordThenReject,
    NoOp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AccountRejection {
    AlreadyImported,
    Deliberate,
}

impl CommandHandler<AccountCommand> for Account {
    type Rejection = AccountRejection;

    fn handle(
        command: &AccountCommand,
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        match command {
            AccountCommand::Import {
                opening_balance,
                provenance,
            } => {
                if context.state().imported {
                    return Err(AccountRejection::AlreadyImported);
                }
                context.record(AccountEvent::AccountStateImported {
                    opening_balance: *opening_balance,
                    provenance: provenance.clone(),
                });
            }
            AccountCommand::CreditThenObserve { amount } => {
                context.record(AccountEvent::Credited { amount: *amount });
                let balance_after_credit = context.state().balance;
                context.record(AccountEvent::BalanceObserved {
                    balance: balance_after_credit,
                });
            }
            AccountCommand::RecordThenReject => {
                context.record(AccountEvent::Credited { amount: 100 });
                return Err(AccountRejection::Deliberate);
            }
            AccountCommand::NoOp => {}
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct ImportedPayload {
    opening_balance: i64,
    source_system: String,
    source_record: String,
    observed_at: String,
    import_batch: String,
}

#[derive(Serialize, Deserialize)]
struct AmountPayload {
    amount: i64,
}

#[derive(Serialize, Deserialize)]
struct BalancePayload {
    balance: i64,
}

struct AccountCodec;

impl EventCodec<Account> for AccountCodec {
    fn encode(
        &self,
        event: &AccountEvent,
        event_id: rostfrei_core::EventId,
    ) -> Result<NewEvent, EventCodecError> {
        let (event_type, payload) = match event {
            AccountEvent::AccountStateImported {
                opening_balance,
                provenance,
            } => (
                "account-state-imported",
                encode_json(&ImportedPayload {
                    opening_balance: *opening_balance,
                    source_system: provenance.source_system.clone(),
                    source_record: provenance.source_record.clone(),
                    observed_at: provenance.observed_at.clone(),
                    import_batch: provenance.import_batch.clone(),
                })?,
            ),
            AccountEvent::Credited { amount } => (
                "account-credited",
                encode_json(&AmountPayload { amount: *amount })?,
            ),
            AccountEvent::BalanceObserved { balance } => (
                "account-balance-observed",
                encode_json(&BalancePayload { balance: *balance })?,
            ),
        };
        NewEvent::new(event_id, event_type, 1, payload).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, error.to_string())
        })
    }

    fn decode(
        &self,
        event: &rostfrei_core::RecordedEvent,
    ) -> Result<AccountEvent, EventCodecError> {
        if event.schema_version() != 1 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "account events support schema version 1",
            ));
        }
        match event.event_type() {
            "account-state-imported" => {
                let payload: ImportedPayload = decode_json(event.payload())?;
                Ok(AccountEvent::AccountStateImported {
                    opening_balance: payload.opening_balance,
                    provenance: ImportProvenance {
                        source_system: payload.source_system,
                        source_record: payload.source_record,
                        observed_at: payload.observed_at,
                        import_batch: payload.import_batch,
                    },
                })
            }
            "account-credited" => {
                let payload: AmountPayload = decode_json(event.payload())?;
                Ok(AccountEvent::Credited {
                    amount: payload.amount,
                })
            }
            "account-balance-observed" => {
                let payload: BalancePayload = decode_json(event.payload())?;
                Ok(AccountEvent::BalanceObserved {
                    balance: payload.balance,
                })
            }
            unknown => Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                format!("unknown account event type {unknown}"),
            )),
        }
    }
}

struct AlternateAccountCodec;

impl EventCodec<Account> for AlternateAccountCodec {
    fn encode(
        &self,
        event: &AccountEvent,
        event_id: rostfrei_core::EventId,
    ) -> Result<NewEvent, EventCodecError> {
        AccountCodec.encode(event, event_id)
    }

    fn decode(&self, event: &RecordedEvent) -> Result<AccountEvent, EventCodecError> {
        AccountCodec.decode(event)
    }
}

#[derive(domain::BoundedContext)]
#[domain(id = "automatic-accounts", label = "Automatic accounts")]
struct AutomaticAccounts;

#[derive(domain::DomainIdentity)]
#[domain(owner = AutomaticAccountRoot)]
#[allow(dead_code)]
struct AutomaticAccountId(String);

#[derive(domain::Entity)]
#[domain(
    id = "automatic-account-root",
    label = "Automatic account",
    owner = AutomaticAccountDefinition
)]
struct AutomaticAccountRoot {
    #[domain(identity)]
    #[allow(dead_code)]
    id: AutomaticAccountId,
    balance: i64,
}

#[derive(domain::Aggregate)]
#[domain(
    id = "automatic-account",
    label = "Automatic account",
    context = AutomaticAccounts,
    root = AutomaticAccountRoot,
    events = [MoneyDeposited]
)]
struct AutomaticAccountDefinition;

#[derive(Clone, Debug, Deserialize, domain::DomainEvent, Eq, PartialEq, Serialize)]
#[domain(id = "money-deposited", label = "Money deposited", schema_version = 2)]
struct MoneyDeposited {
    amount: i64,
}

impl Initialize<AutomaticAccountDefinition> for AutomaticAccountRoot {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: AutomaticAccountId(stream_id.aggregate_id().as_str().to_owned()),
            balance: 0,
        }
    }
}

impl Apply<MoneyDeposited> for AutomaticAccountRoot {
    fn apply(&mut self, event: &MoneyDeposited) {
        self.balance += event.amount;
    }
}

struct DepositMoney {
    amount: i64,
}

impl CommandHandler<DepositMoney> for AutomaticAccountDefinition {
    type Rejection = ();

    fn handle(
        command: &DepositMoney,
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        context.record(MoneyDeposited {
            amount: command.amount,
        });
        Ok(())
    }
}

#[derive(Default)]
struct RecordingAutomaticEventHandler {
    events: Mutex<Vec<MoneyDeposited>>,
}

#[async_trait]
impl DomainEventHandler<MoneyDeposited> for RecordingAutomaticEventHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, MoneyDeposited>,
    ) -> Result<(), DomainEventHandlerError> {
        self.events
            .lock()
            .expect("automatic event recording lock")
            .push(event.event().clone());
        Ok(())
    }
}

struct RecordingDomainEventHandler {
    failure: Option<DomainEventHandlerErrorKind>,
    handled: Mutex<Vec<(AccountEvent, RecordedEvent)>>,
}

impl RecordingDomainEventHandler {
    fn succeeding() -> Self {
        Self {
            failure: None,
            handled: Mutex::new(Vec::new()),
        }
    }

    fn failing(kind: DomainEventHandlerErrorKind) -> Self {
        Self {
            failure: Some(kind),
            handled: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl DomainEventHandler<AccountEvent> for RecordingDomainEventHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, AccountEvent>,
    ) -> Result<(), DomainEventHandlerError> {
        if let Some(kind) = self.failure {
            return Err(DomainEventHandlerError::new(kind, "forced handler result"));
        }
        self.handled
            .lock()
            .expect("handler recording lock")
            .push((event.event().clone(), event.recorded().clone()));
        Ok(())
    }
}

struct ForcedConflictStore {
    inner: InMemoryEventStore,
    remaining_conflicts: AtomicUsize,
    append_attempts: AtomicUsize,
}

impl ForcedConflictStore {
    fn new(remaining_conflicts: usize) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            remaining_conflicts: AtomicUsize::new(remaining_conflicts),
            append_attempts: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl EventStore for ForcedConflictStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(stream_id).await
    }

    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.append_attempts.fetch_add(1, Ordering::Relaxed);
        if self
            .remaining_conflicts
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Conflict,
                "forced test conflict",
            ));
        }
        self.inner.append(stream_id, expected_version, batch).await
    }
}

fn encode_json(value: &impl Serialize) -> Result<Vec<u8>, EventCodecError> {
    serde_json::to_vec(value).map_err(|error| {
        EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
    })
}

fn decode_json<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T, EventCodecError> {
    serde_json::from_slice(payload).map_err(|error| {
        EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
    })
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("Account").expect("valid aggregate type"),
        AggregateId::new(id).expect("valid aggregate id"),
    )
}

fn metadata(stream: &StreamId, operation: &str, command_content: &str) -> ExecutionMetadata {
    ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation).expect("valid operation id"),
        ContentFingerprint::digest(command_content),
    )
}

fn provenance() -> ImportProvenance {
    ImportProvenance {
        source_system: "legacy-ledger".to_owned(),
        source_record: "account-4242".to_owned(),
        observed_at: "2026-08-25T09:30:00Z".to_owned(),
        import_batch: "migration-17".to_owned(),
    }
}

#[tokio::test]
async fn in_memory_store_satisfies_reusable_contract() {
    event_store_contract::run(InMemoryEventStore::new).await;
}

#[tokio::test]
async fn domain_event_handlers_decode_typed_events_preserve_metadata_and_classify_results() {
    let correlation = CorrelationId::new("correlation-1").expect("correlation identity");
    let causation = CausationId::new("command-1").expect("causation identity");
    let event = recorded_account_event("account-credited", 1, br#"{"amount":7}"#)
        .with_correlation_id(correlation.clone())
        .with_causation_id(causation.clone());
    let handler = Arc::new(RecordingDomainEventHandler::succeeding());
    let mut harness = DomainEventHandlerHarness::new();
    harness
        .register_with_codec::<Account, AccountEvent, _, _>(
            "account-credited",
            Arc::new(AccountCodec),
            handler.clone(),
        )
        .expect("handler registration");

    assert_eq!(
        harness.handle(&event).await.expect("handler succeeds"),
        DomainEventDispatchOutcome::Handled
    );
    {
        let records = handler.handled.lock().expect("handler recording lock");
        assert_eq!(records[0].0, AccountEvent::Credited { amount: 7 });
        assert_eq!(records[0].1.correlation_id(), Some(&correlation));
        assert_eq!(records[0].1.causation_id(), Some(&causation));
        assert_eq!(records[0].1.commit_event_ordinal(), 0);
        assert_eq!(records[0].1.commit_event_count(), 1);
    }

    let ignored = recorded_account_event("account-debited", 1, br#"{"amount":2}"#);
    assert_eq!(
        harness
            .handle(&ignored)
            .await
            .expect("unknown event is ignored"),
        DomainEventDispatchOutcome::Ignored
    );
    assert_eq!(
        handler
            .handled
            .lock()
            .expect("handler recording lock")
            .len(),
        1
    );

    for failure_kind in [
        DomainEventHandlerErrorKind::Retryable,
        DomainEventHandlerErrorKind::PermanentlyUnsupported,
        DomainEventHandlerErrorKind::OperatorBlocking,
    ] {
        let mut failing_harness = DomainEventHandlerHarness::new();
        failing_harness
            .register_with_codec::<Account, AccountEvent, _, _>(
                "account-credited",
                Arc::new(AccountCodec),
                Arc::new(RecordingDomainEventHandler::failing(failure_kind)),
            )
            .expect("failing handler registration");
        assert_eq!(
            failing_harness
                .handle(&event)
                .await
                .expect_err("handler failure is explicit")
                .kind(),
            failure_kind
        );
    }
}

#[tokio::test]
async fn expected_domain_events_fail_closed_for_schema_payload_and_registration_conflicts() {
    let handler = Arc::new(RecordingDomainEventHandler::succeeding());
    let mut harness = DomainEventHandlerHarness::new();
    harness
        .register_with_codec::<Account, AccountEvent, _, _>(
            "account-credited",
            Arc::new(AccountCodec),
            handler.clone(),
        )
        .expect("handler registration");
    let conflict = harness
        .register_with_codec::<Account, AccountEvent, _, _>(
            "account-credited",
            Arc::new(AccountCodec),
            handler,
        )
        .expect_err("duplicate registration must fail");
    assert!(matches!(
        conflict,
        DomainEventRegistrationError::Conflict { .. }
    ));
    let aggregate_conflict = harness
        .register_with_codec::<Account, AccountEvent, _, _>(
            "account-balance-observed",
            Arc::new(AlternateAccountCodec),
            Arc::new(RecordingDomainEventHandler::succeeding()),
        )
        .expect_err("one aggregate type must use one model and codec");
    assert!(matches!(
        aggregate_conflict,
        DomainEventRegistrationError::AggregateConflict { .. }
    ));

    let unsupported = recorded_account_event("account-credited", 99, br#"{"amount":1}"#);
    assert_eq!(
        harness
            .handle(&unsupported)
            .await
            .expect_err("unsupported expected schema blocks")
            .kind(),
        DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
    let malformed = recorded_account_event("account-credited", 1, b"not-json");
    assert_eq!(
        harness
            .handle(&malformed)
            .await
            .expect_err("malformed expected payload blocks")
            .kind(),
        DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
}

fn recorded_account_event(event_type: &str, schema_version: u32, payload: &[u8]) -> RecordedEvent {
    let stream = stream("handled-account");
    let metadata = metadata(&stream, "handled-operation", "handled-content");
    RecordedEvent::new(
        stream,
        StreamVersion::new(1),
        metadata.event_id(0),
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        event_type,
        schema_version,
        payload,
    )
    .expect("recorded account event")
}

#[test]
fn given_when_then_exposes_live_state_and_replay_equivalence() {
    let mut history = vec![AccountEvent::AccountStateImported {
        opening_balance: 10,
        provenance: provenance(),
    }];
    let stream = stream("given-account");
    let then = given::<Account, _>(&stream, history.clone())
        .when(&AccountCommand::CreditThenObserve { amount: 5 });

    assert!(then.is_accepted());
    assert_eq!(
        then.events(),
        &[
            AccountEvent::Credited { amount: 5 },
            AccountEvent::BalanceObserved { balance: 15 },
        ]
    );
    assert_eq!(then.state().balance, 15);
    assert_eq!(then.state().last_observed_balance, Some(15));

    let (live_state, new_events, decision) = then.into_parts();
    assert_eq!(decision, Ok(()));
    history.extend(new_events);
    let replayed = given::<Account, _>(&stream, history);
    assert_eq!(replayed.state(), &live_state);
}

#[tokio::test]
async fn executor_replays_retries_rejections_and_preserves_import_provenance() {
    let stream = stream("imported-account");
    let executor = Executor::with_codec(InMemoryEventStore::new(), AccountCodec);
    let import_metadata = metadata(&stream, "import-operation", "stable-import-command");
    let import = AccountCommand::Import {
        opening_balance: 10,
        provenance: provenance(),
    };

    let imported = executor
        .execute::<Account, _>(import_metadata, &import)
        .await
        .expect("honest NoStream import should succeed");
    assert!(matches!(imported, ExecutionOutcome::Appended(_)));
    let imported_payload: ImportedPayload =
        serde_json::from_slice(imported.events()[0].payload()).expect("valid import payload");
    assert_eq!(imported_payload.source_system, "legacy-ledger");
    assert_eq!(imported_payload.source_record, "account-4242");
    assert_eq!(imported_payload.observed_at, "2026-08-25T09:30:00Z");
    assert_eq!(imported_payload.import_batch, "migration-17");

    let credit_metadata = metadata(&stream, "credit-operation", "credit-five-and-observe")
        .with_correlation_id(
            CorrelationId::new("credit-correlation").expect("credit correlation identity"),
        )
        .with_causation_id(CausationId::new("credit-command").expect("credit causation identity"));
    let credit = AccountCommand::CreditThenObserve { amount: 5 };
    let credited = executor
        .execute::<Account, _>(credit_metadata.clone(), &credit)
        .await
        .expect("command should observe replayed imported balance");
    assert_eq!(credited.events().len(), 2);
    let observed = AccountCodec
        .decode(&credited.events()[1])
        .expect("recorded event should decode");
    assert_eq!(observed, AccountEvent::BalanceObserved { balance: 15 });

    let retried = executor
        .execute::<Account, _>(credit_metadata.clone(), &credit)
        .await
        .expect("same operation should be an exact replay");
    assert!(matches!(retried, ExecutionOutcome::ExactReplay(_)));
    assert_eq!(retried.events(), credited.events());

    let changed_causation = metadata(&stream, "credit-operation", "credit-five-and-observe")
        .with_correlation_id(
            CorrelationId::new("credit-correlation").expect("credit correlation identity"),
        )
        .with_causation_id(
            CausationId::new("different-command").expect("different causation identity"),
        );
    let metadata_conflict = executor
        .execute::<Account, _>(changed_causation, &credit)
        .await
        .expect_err("metadata is part of exact operation identity");
    assert!(matches!(
        metadata_conflict,
        ExecutionError::Store(ref error)
            if error.kind() == EventStoreErrorKind::IdentityConflict
    ));

    let before_rejection = executor
        .store()
        .load(&stream)
        .await
        .expect("load should succeed");
    let rejection = executor
        .execute::<Account, _>(
            metadata(&stream, "rejected-operation", "record-then-reject"),
            &AccountCommand::RecordThenReject,
        )
        .await
        .expect_err("domain rejection should be returned");
    assert!(matches!(
        rejection,
        ExecutionError::Rejected(AccountRejection::Deliberate)
    ));
    assert_eq!(
        executor
            .store()
            .load(&stream)
            .await
            .expect("load should succeed"),
        before_rejection
    );
}

#[tokio::test]
async fn executor_uses_derived_json_events_without_codec_configuration() {
    let stream = StreamId::new(
        AggregateType::new(AutomaticAccountDefinition::aggregate_type())
            .expect("valid compiled aggregate type"),
        AggregateId::new("automatic-account-1").expect("valid aggregate id"),
    );
    let executor = Executor::new(InMemoryEventStore::new());

    let first = executor
        .execute::<AutomaticAccountDefinition, _>(
            metadata(&stream, "automatic-deposit-1", "deposit-seven"),
            &DepositMoney { amount: 7 },
        )
        .await
        .expect("default JSON event encoding should succeed");
    assert_eq!(first.events()[0].event_type(), "money-deposited");
    assert_eq!(first.events()[0].schema_version(), 2);
    assert_eq!(first.events()[0].payload(), br#"{"amount":7}"#);

    let handler = Arc::new(RecordingAutomaticEventHandler::default());
    let mut harness = DomainEventHandlerHarness::new();
    harness
        .register::<AutomaticAccountDefinition, MoneyDeposited, _>(
            "money-deposited",
            handler.clone(),
        )
        .expect("default JSON event handler registration should succeed");
    assert_eq!(
        harness
            .handle(&first.events()[0])
            .await
            .expect("default JSON event handling should succeed"),
        DomainEventDispatchOutcome::Handled
    );
    assert_eq!(
        handler
            .events
            .lock()
            .expect("automatic event recording lock")
            .as_slice(),
        &[MoneyDeposited { amount: 7 }]
    );

    let second = executor
        .execute::<AutomaticAccountDefinition, _>(
            metadata(&stream, "automatic-deposit-2", "deposit-three"),
            &DepositMoney { amount: 3 },
        )
        .await
        .expect("derived JSON events should replay without codec configuration");
    assert_eq!(second.events()[0].payload(), br#"{"amount":3}"#);
}

#[tokio::test]
async fn executor_fails_closed_for_unknown_and_malformed_events() {
    let unknown_stream = stream("unknown-codec");
    let unknown_store = InMemoryEventStore::new();
    append_raw(
        &unknown_store,
        &unknown_stream,
        "seed-unknown",
        "unknown-event",
        b"{}",
    )
    .await;
    let unknown_executor = Executor::with_codec(unknown_store, AccountCodec);
    let error = unknown_executor
        .execute::<Account, _>(
            metadata(&unknown_stream, "after-unknown", "noop"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("unknown event types must fail replay");
    assert!(matches!(
        error,
        ExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::UnknownEventType
    ));

    let malformed_stream = stream("malformed-codec");
    let malformed_store = InMemoryEventStore::new();
    append_raw(
        &malformed_store,
        &malformed_stream,
        "seed-malformed",
        "account-credited",
        b"not-json",
    )
    .await;
    let malformed_executor = Executor::with_codec(malformed_store, AccountCodec);
    let error = malformed_executor
        .execute::<Account, _>(
            metadata(&malformed_stream, "after-malformed", "noop"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("malformed payloads must fail replay");
    assert!(matches!(
        error,
        ExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::MalformedPayload
    ));

    let unknown_version_stream = stream("unknown-version-codec");
    let unknown_version_store = InMemoryEventStore::new();
    append_raw_version(
        &unknown_version_store,
        &unknown_version_stream,
        "seed-unknown-version",
        "account-credited",
        99,
        br#"{"amount":1}"#,
    )
    .await;
    let unknown_version_executor = Executor::with_codec(unknown_version_store, AccountCodec);
    let error = unknown_version_executor
        .execute::<Account, _>(
            metadata(&unknown_version_stream, "after-unknown-version", "noop"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("unknown event schema versions must fail replay");
    assert!(matches!(
        error,
        ExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::UnsupportedSchemaVersion
    ));
}

#[tokio::test]
async fn executor_retries_conflicts_with_a_hard_bound() {
    let successful_stream = stream("retry-conflict");
    let successful = Executor::with_codec(ForcedConflictStore::new(1), AccountCodec)
        .with_max_conflict_retries(1);
    let outcome = successful
        .execute::<Account, _>(
            metadata(&successful_stream, "retry-once", "import"),
            &AccountCommand::Import {
                opening_balance: 1,
                provenance: provenance(),
            },
        )
        .await
        .expect("executor should retry one optimistic conflict");
    assert!(matches!(outcome, ExecutionOutcome::Appended(_)));
    assert_eq!(
        successful.store().append_attempts.load(Ordering::Relaxed),
        2
    );

    let exhausted_stream = stream("exhaust-conflict");
    let exhausted = Executor::with_codec(ForcedConflictStore::new(3), AccountCodec)
        .with_max_conflict_retries(2);
    let error = exhausted
        .execute::<Account, _>(
            metadata(&exhausted_stream, "retry-three", "import"),
            &AccountCommand::Import {
                opening_balance: 1,
                provenance: provenance(),
            },
        )
        .await
        .expect_err("executor must stop at its configured retry bound");
    assert!(matches!(
        error,
        ExecutionError::Store(ref store_error)
            if store_error.kind() == EventStoreErrorKind::Conflict
    ));
    assert_eq!(exhausted.store().append_attempts.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn capacity_failure_is_atomic() {
    let store = InMemoryEventStore::with_capacity(1);
    let stream = stream("capacity");
    let metadata = metadata(&stream, "capacity-operation", "two-events");
    let batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![
            NewEvent::new(
                metadata.event_id(0),
                "account-credited",
                1,
                br#"{"amount":1}"#,
            )
            .expect("valid event"),
            NewEvent::new(
                metadata.event_id(1),
                "account-credited",
                1,
                br#"{"amount":2}"#,
            )
            .expect("valid event"),
        ],
    )
    .expect("non-empty batch");
    let error = store
        .append(&stream, ExpectedVersion::NoStream, batch)
        .await
        .expect_err("capacity should reject the whole batch");
    assert_eq!(error.kind(), EventStoreErrorKind::CapacityExhausted);
    assert!(store
        .load(&stream)
        .await
        .expect("load should succeed")
        .is_empty());
}

#[test]
fn envelopes_and_identities_reject_invalid_values() {
    assert!(AggregateType::new("").is_err());
    assert!(AggregateId::new(" account ").is_err());
    assert!(OperationId::new("\n").is_err());
    assert!(ContentFingerprint::from_hex("not-a-fingerprint").is_err());
    assert_eq!(
        EventBatch::new(
            rostfrei_core::CommitId::new("commit").expect("valid id"),
            OperationId::new("operation").expect("valid id"),
            ContentFingerprint::digest("content"),
            Vec::new(),
        ),
        Err(EnvelopeError::EmptyBatch)
    );
}

async fn append_raw(
    store: &InMemoryEventStore,
    stream: &StreamId,
    operation: &str,
    event_type: &str,
    payload: &[u8],
) {
    append_raw_version(store, stream, operation, event_type, 1, payload).await;
}

async fn append_raw_version(
    store: &InMemoryEventStore,
    stream: &StreamId,
    operation: &str,
    event_type: &str,
    schema_version: u32,
    payload: &[u8],
) {
    let metadata = metadata(
        stream,
        operation,
        payload.escape_ascii().to_string().as_str(),
    );
    let event = NewEvent::new(metadata.event_id(0), event_type, schema_version, payload)
        .expect("valid raw event envelope");
    let batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![event],
    )
    .expect("non-empty batch");
    store
        .append(stream, ExpectedVersion::NoStream, batch)
        .await
        .expect("raw seed append should succeed");
}
