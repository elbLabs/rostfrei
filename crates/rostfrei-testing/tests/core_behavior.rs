use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateInstance, AggregateType, AppendOutcome, CommandExecutionError,
    CommandHandler, CommandOutcome, CommandReceipt, CommandResult, CommittedDomainEvent,
    ContentFingerprint, DomainEventDispatchOutcome, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EnvelopeError, EventBatch,
    EventCodec, EventCodecError, EventCodecErrorKind, EventHistory, EventStore, EventStoreError,
    EventStoreErrorKind, EventTransaction, ExecutionMetadata, Executor, ExpectedVersion,
    InMemoryEventStore, MAX_EVENTS_PER_BATCH, NewEvent, OperationId, RecordedEvent,
    SimulationDecision, StreamId, StreamVersion, TransactionParticipant,
};
use rostfrei_domain_runtime::{Apply, Initialize};
use rostfrei_messaging_core::{CausationId, CorrelationId};
use rostfrei_testing::{DomainEventHandlerHarness, event_store_contract, given};
use serde::{Deserialize, Serialize};

type TestResult<T = ()> = Result<T, TestError>;

#[derive(Debug)]
enum TestError {
    InvalidFixture {
        context: &'static str,
        message: String,
    },
    Operation {
        context: &'static str,
        message: String,
    },
}

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFixture { context, message } => {
                write!(formatter, "{context}: invalid test fixture: {message}")
            }
            Self::Operation { context, message } => {
                write!(formatter, "{context}: test operation failed: {message}")
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
            AccountEvent::Credited { amount } => {
                state.balance = state.balance.wrapping_add(*amount);
            }
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
    CreditMany {
        event_count: usize,
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
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        match command {
            AccountCommand::Import {
                opening_balance,
                provenance,
            } => {
                if aggregate.state().imported {
                    return Err(AccountRejection::AlreadyImported);
                }
                aggregate.raise(AccountEvent::AccountStateImported {
                    opening_balance: *opening_balance,
                    provenance: provenance.clone(),
                });
            }
            AccountCommand::CreditThenObserve { amount } => {
                aggregate.raise(AccountEvent::Credited { amount: *amount });
                let balance_after_credit = aggregate.state().balance;
                aggregate.raise(AccountEvent::BalanceObserved {
                    balance: balance_after_credit,
                });
            }
            AccountCommand::CreditMany { event_count } => {
                for _ in 0..*event_count {
                    aggregate.raise(AccountEvent::Credited { amount: 1 });
                }
            }
            AccountCommand::RecordThenReject => {
                aggregate.raise(AccountEvent::Credited { amount: 100 });
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
#[allow(dead_code)]
struct AutomaticAccountId(String);

#[derive(domain::Entity)]
#[domain(id = "automatic-account-root", label = "Automatic account")]
struct AutomaticAccountRoot {
    #[allow(dead_code)]
    id: AutomaticAccountId,
    balance: i64,
}

impl domain::EntityDefinition for AutomaticAccountRoot {
    type Owner = AutomaticAccountDefinition;
    type Identity = AutomaticAccountId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, domain::DomainEvent, Eq, PartialEq, Serialize)]
#[domain(id = "money-deposited", label = "Money deposited", schema_version = 2)]
struct MoneyDeposited {
    amount: i64,
}

#[derive(domain::AggregateEvents)]
enum AutomaticAccountEvents {
    MoneyDeposited(MoneyDeposited),
}

#[derive(domain::Aggregate)]
#[domain(id = "automatic-account", label = "Automatic account")]
struct AutomaticAccountDefinition;

impl domain::AggregateDefinition for AutomaticAccountDefinition {
    type Context = AutomaticAccounts;
    type Root = AutomaticAccountRoot;
    type Event = AutomaticAccountEvents;
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
        self.balance = self.balance.wrapping_add(event.amount);
    }
}

struct DepositMoney {
    amount: i64,
}

impl CommandHandler<DepositMoney> for AutomaticAccountDefinition {
    type Rejection = ();

    fn handle(
        command: &DepositMoney,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.raise(MoneyDeposited {
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
            .map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    format!("automatic event recording lock was poisoned: {error}"),
                )
            })?
            .push(event.event().clone());
        Ok(())
    }
}

struct RecordingDomainEventHandler {
    failure: Option<DomainEventHandlerErrorKind>,
    handled: Mutex<Vec<(AccountEvent, RecordedEvent)>>,
}

impl RecordingDomainEventHandler {
    const fn succeeding() -> Self {
        Self {
            failure: None,
            handled: Mutex::new(Vec::new()),
        }
    }

    const fn failing(kind: DomainEventHandlerErrorKind) -> Self {
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
            .map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    format!("handler recording lock was poisoned: {error}"),
                )
            })?
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
impl EventHistory for ForcedConflictStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for ForcedConflictStore {
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

struct EmptyEventHistory;

struct AppendOnlyStore(InMemoryEventStore);

#[async_trait]
impl EventHistory for AppendOnlyStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for AppendOnlyStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.0.append(stream_id, expected_version, batch).await
    }
}

#[async_trait]
impl EventHistory for EmptyEventHistory {
    async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Ok(Vec::new())
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

fn stream(id: &str) -> TestResult<StreamId> {
    let aggregate_type = AggregateType::new("Account")
        .map_err(|error| fixture_error("account aggregate type", error))?;
    let aggregate_id =
        AggregateId::new(id).map_err(|error| fixture_error("account aggregate ID", error))?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

fn metadata(
    stream: &StreamId,
    operation: &str,
    command_content: &str,
) -> TestResult<ExecutionMetadata> {
    let operation_id = OperationId::new(operation)
        .map_err(|error| fixture_error("account operation ID", error))?;
    Ok(ExecutionMetadata::new(
        stream.clone(),
        operation_id,
        ContentFingerprint::digest(command_content),
    ))
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
async fn in_memory_store_satisfies_reusable_contracts() {
    event_store_contract::run(InMemoryEventStore::new).await;
    event_store_contract::run_atomic_multi_stream_transactions(InMemoryEventStore::new).await;
}

#[tokio::test]
async fn default_transaction_adapter_satisfies_reusable_base_contract() {
    event_store_contract::run(|| AppendOnlyStore(InMemoryEventStore::new())).await;
}

#[tokio::test]
async fn reusable_contract_reports_errors_and_legacy_wrapper_fails_closed() {
    let result = event_store_contract::try_run(|| InMemoryEventStore::with_capacity(0)).await;
    assert!(matches!(
        result,
        Err(event_store_contract::ContractTestError::Store {
            context: "NoStream should append to an absent stream",
            ..
        })
    ));

    let assertion = tokio::spawn(async {
        event_store_contract::run(|| InMemoryEventStore::with_capacity(0)).await;
    })
    .await;
    assert!(matches!(assertion, Err(error) if error.is_panic()));
}

#[tokio::test]
async fn default_transaction_adapter_rejects_all_transactions_and_has_no_receipts() {
    let stream = stream("default-transaction-adapter")
        .expect("valid default transaction adapter stream fixture");
    let operation = "default-transaction-operation";
    let transaction_metadata = metadata(&stream, operation, "default-transaction-content")
        .expect("valid default transaction metadata fixture");
    let batch = EventBatch::new(
        transaction_metadata.commit_id().clone(),
        transaction_metadata.operation_id().clone(),
        transaction_metadata.operation_fingerprint(),
        vec![
            NewEvent::new(
                transaction_metadata.event_id(0),
                "default-transaction-event",
                1,
                b"event",
            )
            .expect("valid event"),
        ],
    )
    .expect("valid batch");
    let store = AppendOnlyStore(InMemoryEventStore::new());
    let transactions = [
        EventTransaction::new(
            transaction_metadata.operation_id().clone(),
            transaction_metadata.operation_fingerprint(),
            vec![TransactionParticipant::new(
                stream.clone(),
                ExpectedVersion::NoStream,
                Some(batch.clone()),
            )],
        ),
        EventTransaction::new(
            OperationId::new("default-read-only-primary").expect("valid operation identity"),
            ContentFingerprint::digest("default-read-only-primary"),
            vec![TransactionParticipant::new(
                stream.clone(),
                ExpectedVersion::NoStream,
                None,
            )],
        ),
        EventTransaction::new(
            OperationId::new("default-empty-transaction").expect("valid operation identity"),
            ContentFingerprint::digest("default-empty-transaction"),
            Vec::new(),
        ),
    ];

    for transaction in transactions {
        let error = store
            .append_transaction(transaction)
            .await
            .expect_err("an adapter without transaction support must reject every transaction");
        assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
    }
    assert!(
        store
            .load(&stream)
            .await
            .expect("rejected transaction stream should load")
            .is_empty()
    );
    assert!(
        store
            .load_transaction_receipt(&stream, transaction_metadata.operation_id())
            .await
            .expect("the default receipt lookup should succeed")
            .is_none()
    );
}

#[tokio::test]
async fn domain_event_handlers_decode_typed_events_preserve_metadata_and_classify_results() {
    let correlation = CorrelationId::new("correlation-1").expect("correlation identity");
    let causation = CausationId::new("command-1").expect("causation identity");
    let event = recorded_account_event("account-credited", 1, br#"{"amount":7}"#)
        .expect("valid credited account event fixture")
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
        drop(records);
    }

    let ignored = recorded_account_event("account-debited", 1, br#"{"amount":2}"#)
        .expect("valid unregistered account event fixture");
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

    let unsupported = recorded_account_event("account-credited", 99, br#"{"amount":1}"#)
        .expect("valid unsupported-schema account event fixture");
    assert_eq!(
        harness
            .handle(&unsupported)
            .await
            .expect_err("unsupported expected schema blocks")
            .kind(),
        DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
    let malformed = recorded_account_event("account-credited", 1, b"not-json")
        .expect("valid malformed-payload account event fixture");
    assert_eq!(
        harness
            .handle(&malformed)
            .await
            .expect_err("malformed expected payload blocks")
            .kind(),
        DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
}

fn recorded_account_event(
    event_type: &str,
    schema_version: u32,
    payload: &[u8],
) -> TestResult<RecordedEvent> {
    let stream = stream("handled-account")?;
    let metadata = metadata(&stream, "handled-operation", "handled-content")?;
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
    .map_err(|error| fixture_error("recorded account event", error))
}

#[test]
fn given_when_then_exposes_live_state_and_replay_equivalence() {
    let mut history = vec![AccountEvent::AccountStateImported {
        opening_balance: 10,
        provenance: provenance(),
    }];
    let stream = stream("given-account").expect("valid given account stream fixture");
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
    let stream = stream("imported-account").expect("valid imported account stream fixture");
    let executor = Executor::with_codec(InMemoryEventStore::new(), AccountCodec);
    let import_metadata = metadata(&stream, "import-operation", "stable-import-command")
        .expect("valid import execution metadata fixture");
    let import = AccountCommand::Import {
        opening_balance: 10,
        provenance: provenance(),
    };

    let imported = executor
        .execute::<Account, _>(import_metadata, &import)
        .await
        .expect("honest NoStream import should succeed");
    let CommandOutcome::Accepted(imported) = imported else {
        panic!("honest NoStream import should be accepted");
    };
    assert!(matches!(imported, CommandReceipt::Appended(_)));
    let imported_payload: ImportedPayload =
        serde_json::from_slice(imported.events()[0].payload()).expect("valid import payload");
    assert_eq!(imported_payload.source_system, "legacy-ledger");
    assert_eq!(imported_payload.source_record, "account-4242");
    assert_eq!(imported_payload.observed_at, "2026-08-25T09:30:00Z");
    assert_eq!(imported_payload.import_batch, "migration-17");

    let credit_metadata = metadata(&stream, "credit-operation", "credit-five-and-observe")
        .expect("valid credit execution metadata fixture")
        .with_correlation_id(
            CorrelationId::new("credit-correlation").expect("credit correlation identity"),
        )
        .with_causation_id(CausationId::new("credit-command").expect("credit causation identity"));
    let credit = AccountCommand::CreditThenObserve { amount: 5 };
    let credited = executor
        .execute::<Account, _>(credit_metadata.clone(), &credit)
        .await
        .expect("command should observe replayed imported balance");
    let CommandOutcome::Accepted(credited) = credited else {
        panic!("credit should be accepted");
    };
    assert_eq!(credited.events().len(), 2);
    let observed = AccountCodec
        .decode(&credited.events()[1])
        .expect("recorded event should decode");
    assert_eq!(observed, AccountEvent::BalanceObserved { balance: 15 });

    let retried = executor
        .execute::<Account, _>(credit_metadata.clone(), &credit)
        .await
        .expect("same operation should be an exact replay");
    let CommandOutcome::Accepted(retried) = retried else {
        panic!("exact retry should remain accepted");
    };
    assert!(matches!(retried, CommandReceipt::ExactReplay(_)));
    assert_eq!(retried.events(), credited.events());

    let changed_causation = metadata(&stream, "credit-operation", "credit-five-and-observe")
        .expect("valid conflicting execution metadata fixture")
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
        CommandExecutionError::Store(ref error)
            if error.kind() == EventStoreErrorKind::IdentityConflict
    ));

    let before_rejection = executor
        .store()
        .load(&stream)
        .await
        .expect("load should succeed");
    let rejection = executor
        .execute::<Account, _>(
            metadata(&stream, "rejected-operation", "record-then-reject")
                .expect("valid rejected execution metadata fixture"),
            &AccountCommand::RecordThenReject,
        )
        .await
        .expect("domain rejection is a completed command outcome");
    assert!(matches!(
        rejection,
        CommandOutcome::Rejected(AccountRejection::Deliberate)
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
async fn executor_rejects_commands_that_exceed_the_atomic_commit_limit() {
    let stream = stream("oversized-command").expect("valid oversized command stream fixture");
    let executor = Executor::with_codec(InMemoryEventStore::new(), AccountCodec);
    let event_count = MAX_EVENTS_PER_BATCH.saturating_add(1);

    let error = executor
        .execute::<Account, _>(
            metadata(
                &stream,
                "oversized-command-operation",
                "oversized-command-content",
            )
            .expect("valid oversized command metadata fixture"),
            &AccountCommand::CreditMany { event_count },
        )
        .await
        .expect_err("a command exceeding the atomic commit limit must fail");
    let CommandExecutionError::Store(error) = error else {
        panic!("an oversized command must fail as an event-store request");
    };
    assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
    assert_eq!(
        error.message(),
        format!(
            "event batch contains {event_count} domain events, exceeding the {MAX_EVENTS_PER_BATCH}-event atomic commit limit; split the work across commands"
        )
    );
    assert!(
        executor
            .store()
            .load(&stream)
            .await
            .expect("oversized command stream should load")
            .is_empty()
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
            metadata(&stream, "automatic-deposit-1", "deposit-seven")
                .expect("valid first automatic deposit metadata fixture"),
            &DepositMoney { amount: 7 },
        )
        .await
        .expect("default JSON event encoding should succeed");
    let CommandOutcome::Accepted(first) = first else {
        panic!("deposit should be accepted");
    };
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
            metadata(&stream, "automatic-deposit-2", "deposit-three")
                .expect("valid second automatic deposit metadata fixture"),
            &DepositMoney { amount: 3 },
        )
        .await
        .expect("derived JSON events should replay without codec configuration");
    let CommandOutcome::Accepted(second) = second else {
        panic!("deposit should be accepted");
    };
    assert_eq!(second.events()[0].payload(), br#"{"amount":3}"#);
}

#[tokio::test]
async fn executor_returns_an_accepted_no_events_receipt_without_appending() {
    let stream = stream("no-op-execution").expect("valid no-op execution stream fixture");
    let executor = Executor::with_codec(InMemoryEventStore::new(), AccountCodec);

    let result: CommandResult<AccountRejection> = executor
        .execute::<Account, _>(
            metadata(&stream, "no-op-execution", "no-op")
                .expect("valid no-op execution metadata fixture"),
            &AccountCommand::NoOp,
        )
        .await;

    assert_eq!(
        result.expect("no-op execution should complete"),
        CommandOutcome::Accepted(CommandReceipt::NoEvents)
    );
    assert!(
        executor
            .store()
            .load(&stream)
            .await
            .expect("load no-op history")
            .is_empty()
    );
}

#[tokio::test]
async fn simulation_replays_history_and_returns_encoded_predictions_without_appending() {
    let stream = stream("simulated-account").expect("valid simulated account stream fixture");
    let store = InMemoryEventStore::new();
    let executor = Executor::with_codec(store.clone(), AccountCodec);
    let seed = executor
        .execute::<Account, _>(
            metadata(&stream, "simulation-seed", "import-ten")
                .expect("valid simulation seed metadata fixture"),
            &AccountCommand::Import {
                opening_balance: 10,
                provenance: provenance(),
            },
        )
        .await
        .expect("simulation history seed should append");
    assert!(matches!(
        seed,
        CommandOutcome::Accepted(CommandReceipt::Appended(_))
    ));
    let history_before = store.load(&stream).await.expect("load seeded history");
    let simulation_metadata = metadata(
        &stream,
        "simulated-credit",
        "simulated-credit-five-and-observe",
    )
    .expect("valid credit simulation metadata fixture");

    let outcome = executor
        .simulate::<Account, _>(
            simulation_metadata.clone(),
            &AccountCommand::CreditThenObserve { amount: 5 },
        )
        .await
        .expect("simulation should succeed");

    assert_eq!(outcome.base_version(), StreamVersion::new(1));
    let SimulationDecision::Accepted(events) = outcome.decision() else {
        panic!("credit simulation should be accepted");
    };
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_id(), &simulation_metadata.event_id(0));
    assert_eq!(events[1].event_id(), &simulation_metadata.event_id(1));
    assert_eq!(events[0].event_type(), "account-credited");
    assert_eq!(events[1].event_type(), "account-balance-observed");
    let credited: AmountPayload =
        serde_json::from_slice(events[0].payload()).expect("predicted credit payload");
    let observed: BalancePayload =
        serde_json::from_slice(events[1].payload()).expect("predicted observation payload");
    assert_eq!(credited.amount, 5);
    assert_eq!(observed.balance, 15);
    assert_eq!(
        store.load(&stream).await.expect("load after simulation"),
        history_before
    );
}

#[tokio::test]
async fn simulation_returns_typed_rejection_and_discards_pending_events_without_appending() {
    let stream = stream("rejected-simulation").expect("valid rejected simulation stream fixture");
    let executor = Executor::with_codec(ForcedConflictStore::new(1), AccountCodec);

    let outcome = executor
        .simulate::<Account, _>(
            metadata(&stream, "simulated-rejection", "record-then-reject")
                .expect("valid rejected simulation metadata fixture"),
            &AccountCommand::RecordThenReject,
        )
        .await
        .expect("a domain rejection is a successful simulation");

    assert_eq!(outcome.base_version(), StreamVersion::ZERO);
    assert_eq!(
        outcome.decision(),
        &SimulationDecision::Rejected(AccountRejection::Deliberate)
    );
    assert_eq!(executor.store().append_attempts.load(Ordering::Relaxed), 0);
    assert!(
        executor
            .store()
            .load(&stream)
            .await
            .expect("rejected simulation history")
            .is_empty()
    );
}

#[tokio::test]
async fn simulation_accepts_zero_events_with_read_only_object_safe_history() {
    let stream = stream("no-op-simulation").expect("valid no-op simulation stream fixture");
    let history: Arc<dyn EventHistory> = Arc::new(EmptyEventHistory);
    let executor = Executor::with_codec(history, AccountCodec);

    let outcome = executor
        .simulate::<Account, _>(
            metadata(&stream, "simulated-no-op", "no-op")
                .expect("valid no-op simulation metadata fixture"),
            &AccountCommand::NoOp,
        )
        .await
        .expect("zero-event simulation should succeed");

    assert_eq!(outcome.base_version(), StreamVersion::ZERO);
    assert!(matches!(
        outcome.decision(),
        SimulationDecision::Accepted(events) if events.is_empty()
    ));
}

#[tokio::test]
async fn executor_fails_closed_for_unknown_and_malformed_events() {
    let unknown_stream = stream("unknown-codec").expect("valid unknown-codec stream fixture");
    let unknown_store = InMemoryEventStore::new();
    append_raw(
        &unknown_store,
        &unknown_stream,
        "seed-unknown",
        "unknown-event",
        b"{}",
    )
    .await
    .expect("unknown-event history seed should append");
    let unknown_executor = Executor::with_codec(unknown_store, AccountCodec);
    let error = unknown_executor
        .execute::<Account, _>(
            metadata(&unknown_stream, "after-unknown", "noop")
                .expect("valid post-unknown-event metadata fixture"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("unknown event types must fail replay");
    assert!(matches!(
        error,
        CommandExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::UnknownEventType
    ));

    let malformed_stream = stream("malformed-codec").expect("valid malformed-codec stream fixture");
    let malformed_store = InMemoryEventStore::new();
    append_raw(
        &malformed_store,
        &malformed_stream,
        "seed-malformed",
        "account-credited",
        b"not-json",
    )
    .await
    .expect("malformed-event history seed should append");
    let malformed_executor = Executor::with_codec(malformed_store, AccountCodec);
    let error = malformed_executor
        .execute::<Account, _>(
            metadata(&malformed_stream, "after-malformed", "noop")
                .expect("valid post-malformed-event metadata fixture"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("malformed payloads must fail replay");
    assert!(matches!(
        error,
        CommandExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::MalformedPayload
    ));

    let unknown_version_stream =
        stream("unknown-version-codec").expect("valid unknown-version-codec stream fixture");
    let unknown_version_store = InMemoryEventStore::new();
    append_raw_version(
        &unknown_version_store,
        &unknown_version_stream,
        "seed-unknown-version",
        "account-credited",
        99,
        br#"{"amount":1}"#,
    )
    .await
    .expect("unknown-version history seed should append");
    let unknown_version_executor = Executor::with_codec(unknown_version_store, AccountCodec);
    let error = unknown_version_executor
        .execute::<Account, _>(
            metadata(&unknown_version_stream, "after-unknown-version", "noop")
                .expect("valid post-unknown-version metadata fixture"),
            &AccountCommand::NoOp,
        )
        .await
        .expect_err("unknown event schema versions must fail replay");
    assert!(matches!(
        error,
        CommandExecutionError::Codec(ref codec_error)
            if codec_error.kind() == EventCodecErrorKind::UnsupportedSchemaVersion
    ));
}

#[tokio::test]
async fn executor_retries_conflicts_with_a_hard_bound() {
    let successful_stream = stream("retry-conflict").expect("valid retry stream fixture");
    let successful = Executor::with_codec(ForcedConflictStore::new(1), AccountCodec)
        .with_max_conflict_retries(1);
    let outcome = successful
        .execute::<Account, _>(
            metadata(&successful_stream, "retry-once", "import")
                .expect("valid retry execution metadata fixture"),
            &AccountCommand::Import {
                opening_balance: 1,
                provenance: provenance(),
            },
        )
        .await
        .expect("executor should retry one optimistic conflict");
    assert!(matches!(
        outcome,
        CommandOutcome::Accepted(CommandReceipt::Appended(_))
    ));
    assert_eq!(
        successful.store().append_attempts.load(Ordering::Relaxed),
        2
    );

    let exhausted_stream =
        stream("exhaust-conflict").expect("valid exhausted-retry stream fixture");
    let exhausted = Executor::with_codec(ForcedConflictStore::new(3), AccountCodec)
        .with_max_conflict_retries(2);
    let error = exhausted
        .execute::<Account, _>(
            metadata(&exhausted_stream, "retry-three", "import")
                .expect("valid exhausted-retry execution metadata fixture"),
            &AccountCommand::Import {
                opening_balance: 1,
                provenance: provenance(),
            },
        )
        .await
        .expect_err("executor must stop at its configured retry bound");
    assert!(matches!(
        error,
        CommandExecutionError::Store(ref store_error)
            if store_error.kind() == EventStoreErrorKind::Conflict
    ));
    assert_eq!(exhausted.store().append_attempts.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn capacity_failure_is_atomic() {
    let store = InMemoryEventStore::with_capacity(1);
    let stream = stream("capacity").expect("valid capacity stream fixture");
    let metadata = metadata(&stream, "capacity-operation", "two-events")
        .expect("valid capacity execution metadata fixture");
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
    assert!(
        store
            .load(&stream)
            .await
            .expect("load should succeed")
            .is_empty()
    );
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
) -> TestResult {
    append_raw_version(store, stream, operation, event_type, 1, payload).await
}

async fn append_raw_version(
    store: &InMemoryEventStore,
    stream: &StreamId,
    operation: &str,
    event_type: &str,
    schema_version: u32,
    payload: &[u8],
) -> TestResult {
    let metadata = metadata(
        stream,
        operation,
        payload.escape_ascii().to_string().as_str(),
    )?;
    let event = NewEvent::new(metadata.event_id(0), event_type, schema_version, payload)
        .map_err(|error| fixture_error("raw event envelope", error))?;
    let batch = EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        vec![event],
    )
    .map_err(|error| fixture_error("raw event batch", error))?;
    store
        .append(stream, ExpectedVersion::NoStream, batch)
        .await
        .map_err(|error| TestError::Operation {
            context: "raw seed append",
            message: error.to_string(),
        })?;
    Ok(())
}
