//! Core-only behavioral controls while service/harness integration is staged separately.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]

use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::BoundedContextName;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct Counter;
impl Aggregate for Counter {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "core-policy";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(state: &mut i64, event: &Added) {
        *state = state.saturating_add(event.0);
    }
}

struct Added(i64);
impl Event for Added {
    fn event_type(&self) -> &'static str {
        "added"
    }
    fn schema_version(&self) -> u32 {
        2
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        Ok(self.0.to_string().into_bytes())
    }
    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        serde_json::from_slice(event.payload())
            .map(Self)
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::MalformedPayload, e.to_string()))
    }
}

fn metadata(id: &str) -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new(id).unwrap(),
        ContentFingerprint::digest(id),
    )
    .with_bounded_context(BoundedContextName::new("core-policy").unwrap())
}
fn stream() -> StreamId {
    StreamId::new(
        AggregateType::new("counter").unwrap(),
        AggregateId::new("one").unwrap(),
    )
}
fn key(store: &InMemoryEventStore, id: &str) -> OutcomeKey {
    OutcomeKey::new(
        store.authority().clone(),
        metadata(id).bounded_context().unwrap().clone(),
        metadata(id).operation_id().clone(),
    )
}
async fn add(store: &InMemoryEventStore, id: &str, expected: ExpectedVersion, value: i64) {
    let op = OperationId::new(id).unwrap();
    let commit = derive_commit_id(&stream(), &op);
    let event = NewEvent::new(
        derive_event_id(&commit, 0),
        "added",
        2,
        value.to_string().into_bytes(),
    )
    .unwrap();
    store
        .append(
            &stream(),
            expected,
            EventBatch::new(commit, op, ContentFingerprint::digest(id), vec![event]).unwrap(),
        )
        .await
        .unwrap();
}

struct Racing {
    store: InMemoryEventStore,
    observations: Arc<Mutex<Vec<i64>>>,
    every_time: bool,
}
#[async_trait]
impl CommandHandler<()> for Racing {
    type Rejection = String;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let counter = execution.load::<Counter>("one").await?;
        let count = {
            let mut seen = self.observations.lock().unwrap();
            seen.push(*counter.state());
            seen.len()
        };
        if self.every_time || count == 1 {
            let expected = if counter.base_version() == StreamVersion::ZERO {
                ExpectedVersion::NoStream
            } else {
                ExpectedVersion::Exact(counter.base_version())
            };
            add(&self.store, &format!("competitor-{count}"), expected, 10).await;
        }
        Ok(CommandDecision::Rejected(format!(
            "observed={}",
            counter.state()
        )))
    }
}

#[tokio::test]
async fn zero_one_and_default_policy_use_fresh_read_guards_and_keep_replay() {
    for retries in [Some(0), Some(1), None] {
        let store = InMemoryEventStore::new();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let handler = Racing {
            store: store.clone(),
            observations: observations.clone(),
            every_time: retries.is_none(),
        };
        let mut executor = CommandExecutor::new(store.clone());
        if let Some(retries) = retries {
            executor =
                executor.with_conflict_retry_policy(Arc::new(BoundedConflictRetries::new(retries)));
        }
        let codec = JsonRejectionCodec::new("string", 1);
        let result = executor
            .execute_with_rejection_codec(&handler, metadata("race"), &(), &codec)
            .await;
        if retries == Some(1) {
            assert_eq!(
                result.unwrap(),
                CommandOutcome::Rejected("observed=10".to_owned())
            );
            assert_eq!(
                executor
                    .execute_with_rejection_codec(&handler, metadata("race"), &(), &codec)
                    .await
                    .unwrap(),
                CommandOutcome::Rejected("observed=10".to_owned())
            );
            assert_eq!(*observations.lock().unwrap(), [0, 10]);
        } else {
            assert!(
                matches!(result, Err(CommandExecutionError::Store(ref e)) if e.kind() == EventStoreErrorKind::Conflict)
            );
            assert_eq!(
                observations.lock().unwrap().len(),
                if retries.is_none() { 4 } else { 1 }
            );
            assert!(matches!(
                store.lookup_outcome(&key(&store, "race")).await.unwrap(),
                OutcomeLookup::Missing
            ));
        }
    }
}

struct Contender {
    reject: bool,
    barrier: Arc<tokio::sync::Barrier>,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl CommandHandler<()> for Contender {
    type Rejection = String;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut counter = execution.load::<Counter>("one").await?;
        self.barrier.wait().await;
        if self.reject {
            Ok(CommandDecision::Rejected("winner".to_owned()))
        } else {
            counter.raise(Added(1));
            Ok(CommandDecision::Accepted)
        }
    }
}
#[tokio::test]
async fn zero_retries_reconciles_opposite_proposals_without_another_evaluation() {
    let store = InMemoryEventStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let accept = Contender {
        reject: false,
        barrier: barrier.clone(),
        calls: calls.clone(),
    };
    let reject = Contender {
        reject: true,
        barrier,
        calls: calls.clone(),
    };
    let executor = CommandExecutor::new(store.clone()).with_max_conflict_retries(0);
    let codec = JsonRejectionCodec::new("string", 1);
    let (a, b) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            executor.execute_with_rejection_codec(&accept, metadata("same"), &(), &codec),
            executor.execute_with_rejection_codec(&reject, metadata("same"), &(), &codec)
        )
    })
    .await
    .unwrap();
    let (a, b) = (a.unwrap(), b.unwrap());
    match (&a, &b) {
        (CommandOutcome::Rejected(a), CommandOutcome::Rejected(b)) => assert_eq!(a, b),
        (CommandOutcome::Accepted(a), CommandOutcome::Accepted(b)) => {
            assert_eq!(a.events(), b.events());
            assert_ne!(a.is_exact_replay(), b.is_exact_replay());
        }
        _ => panic!("contenders did not return the same decision"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.load(&stream()).await.unwrap().len(),
        usize::from(matches!(a, CommandOutcome::Accepted(_)))
    );
    executor
        .execute_with_rejection_codec(&accept, metadata("same"), &(), &codec)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

struct RetrySpy {
    calls: Mutex<Vec<ConflictRetry>>,
    delay: Duration,
}
impl ConflictRetryPolicy for RetrySpy {
    fn after_conflict(&self, retry: ConflictRetry) -> ConflictRetryDecision {
        self.calls.lock().unwrap().push(retry);
        ConflictRetryDecision::RetryAfter(self.delay)
    }
}
struct Refuse(AtomicUsize);
#[async_trait]
impl CommandHandler<()> for Refuse {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Rejected("permanent".to_owned()))
    }
}

#[tokio::test]
async fn supplied_policy_only_sees_unresolved_version_conflicts_and_honors_delay() {
    let store = InMemoryEventStore::new();
    let delay = Duration::from_millis(15);
    let policy = Arc::new(RetrySpy {
        calls: Mutex::new(Vec::new()),
        delay,
    });
    let executor = CommandExecutor::new(store.clone()).with_conflict_retry_policy(policy.clone());
    let codec = JsonRejectionCodec::new("string", 1);
    let handler = Racing {
        store: store.clone(),
        observations: Arc::new(Mutex::new(Vec::new())),
        every_time: false,
    };
    let start = std::time::Instant::now();
    executor
        .execute_with_rejection_codec(&handler, metadata("race"), &(), &codec)
        .await
        .unwrap();
    assert!(start.elapsed() >= delay);
    let calls = policy.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls.first().unwrap().evaluations(), 1);
    assert_eq!(
        calls.first().unwrap().error().kind(),
        EventStoreErrorKind::Conflict
    );
    let refusal = Refuse(AtomicUsize::new(0));
    executor
        .execute_with_rejection_codec(&refusal, metadata("refuse"), &(), &codec)
        .await
        .unwrap();
    executor
        .execute_with_rejection_codec(&refusal, metadata("refuse"), &(), &codec)
        .await
        .unwrap();
    // Changed trusted scope cannot create another namespace for the operation.
    assert!(
        matches!(executor.execute_with_rejection_codec(&refusal, metadata("refuse").with_security_scope(SecurityScope::new("other").unwrap()), &(), &codec).await,
        Err(CommandExecutionError::Store(ref e)) if e.kind() == EventStoreErrorKind::IdentityConflict)
    );
    assert!(matches!(
        executor
            .execute_with_rejection_codec(
                &refusal,
                metadata("refuse"),
                &(),
                &JsonRejectionCodec::new("unknown", 1)
            )
            .await,
        Err(CommandExecutionError::Codec(_))
    ));
    assert!(
        matches!(CommandExecutor::new(store.with_outcome_capacity(2)).with_conflict_retry_policy(policy.clone())
        .execute_with_rejection_codec(&refusal, metadata("capacity"), &(), &codec).await,
        Err(CommandExecutionError::RejectedOutcomePersistence(ref e)) if e.kind() == EventStoreErrorKind::CapacityExhausted)
    );
    assert_eq!(policy.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn scope_binding_survives_expiry_and_authority_mismatch_is_never_absence() {
    let store = InMemoryEventStore::new();
    let handler = Refuse(AtomicUsize::new(0));
    let codec = JsonRejectionCodec::new("string", 1);
    let scoped = metadata("scoped").with_security_scope(SecurityScope::new("tenant-a").unwrap());
    let executor = CommandExecutor::new(store.clone());
    executor
        .execute_with_rejection_codec(&handler, scoped.clone(), &(), &codec)
        .await
        .unwrap();
    let OutcomeLookup::Completed(retained) =
        store.lookup_outcome(&key(&store, "scoped")).await.unwrap()
    else {
        panic!("missing decision")
    };
    store
        .expire_details(&key(&store, "scoped"), retained.marker().revision())
        .await
        .unwrap();
    assert!(matches!(
        executor
            .execute_with_rejection_codec(&handler, scoped.clone(), &(), &codec)
            .await,
        Err(CommandExecutionError::OutcomeRecovery(
            OutcomeRecoveryError::Expired
        ))
    ));
    assert!(
        matches!(executor.execute_with_rejection_codec(&handler, scoped.with_security_scope(SecurityScope::new("tenant-b").unwrap()), &(), &codec).await,
        Err(CommandExecutionError::Store(ref e)) if e.kind() == EventStoreErrorKind::IdentityConflict)
    );
    assert!(
        matches!(executor.execute_with_rejection_codec(&handler, metadata("scoped"), &(), &codec).await,
        Err(CommandExecutionError::Store(ref e)) if e.kind() == EventStoreErrorKind::IdentityConflict)
    );
    let other = InMemoryEventStore::new();
    assert_eq!(
        other
            .lookup_outcome(&key(&store, "scoped"))
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert!(
        other
            .expire_details(&key(&store, "scoped"), retained.marker().revision())
            .await
            .is_err()
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}

struct ScaledCodec {
    scale: i64,
    calls: Arc<AtomicUsize>,
}
impl EventCodec<Counter> for ScaledCodec {
    fn encode(&self, event: &Added, id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(id, "added", 2, event.0.to_le_bytes().to_vec())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string()))
    }
    fn decode(&self, event: &RecordedEvent) -> Result<Added, EventCodecError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match event.schema_version() {
            1 => Added::decode_json(event).map(|v| Added(v.0.saturating_mul(self.scale))),
            2 => <[u8; 8]>::try_from(event.payload())
                .map(|v| Added(i64::from_le_bytes(v)))
                .map_err(|e| {
                    EventCodecError::new(EventCodecErrorKind::MalformedPayload, e.to_string())
                }),
            _ => Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "unsupported",
            )),
        }
    }
}

struct CollidingCounter;
impl Aggregate for CollidingCounter {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "core-policy";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    fn apply(state: &mut i64, event: &Added) {
        *state = state.saturating_add(event.0);
    }
}
struct Observe(Arc<Mutex<Vec<i64>>>);
#[async_trait]
impl DomainEventHandler<Added> for Observe {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, Added>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0.lock().unwrap().push(event.event().0);
        Ok(())
    }
}
struct Increment;
#[async_trait]
impl CommandHandler<()> for Increment {
    type Rejection = ();
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<()> {
        let mut counter = execution.load::<Counter>("one").await?;
        assert_eq!(*counter.state(), 200);
        counter.raise(Added(3));
        Ok(CommandDecision::Accepted)
    }
}

async fn seed_historical(store: &InMemoryEventStore) -> Vec<RecordedEvent> {
    let operation = OperationId::new("historical").unwrap();
    let commit = derive_commit_id(&stream(), &operation);
    let event = NewEvent::new(derive_event_id(&commit, 0), "added", 1, b"2".to_vec()).unwrap();
    store
        .append(
            &stream(),
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest("history"),
                vec![event],
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .into_events()
}

async fn balance(executor: &CommandExecutor<InMemoryEventStore>) -> i64 {
    *executor
        .rehydrate::<Counter>(&stream())
        .await
        .unwrap()
        .state()
}

fn historical_codecs(calls: &Arc<AtomicUsize>) -> Arc<AggregateCodecs> {
    let mut codecs = AggregateCodecs::default();
    codecs
        .register::<Counter, _>(ScaledCodec {
            scale: 100,
            calls: calls.clone(),
        })
        .unwrap();
    assert!(matches!(
        codecs.register::<Counter, _>(ScaledCodec {
            scale: 1,
            calls: calls.clone()
        }),
        Err(CodecRegistrationError::Duplicate { .. })
    ));
    assert!(codecs.register_json::<Counter>().is_err());
    assert!(matches!(
        codecs.register_json::<CollidingCounter>(),
        Err(CodecRegistrationError::ModelConflict { .. })
    ));
    assert!(
        codecs
            .replace::<CollidingCounter, _>(JsonEventCodec)
            .is_err()
    );
    assert!(codecs.resolve::<CollidingCounter>().is_err());
    Arc::new(codecs)
}

#[tokio::test]
async fn shared_registry_preserves_historical_meaning_in_derived_validation_execution_and_dispatch()
{
    let store = InMemoryEventStore::new();
    let original = seed_historical(&store).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let codecs = historical_codecs(&calls);
    assert_eq!(codecs.aggregate_types().count(), 1);
    codecs.validate_history(&stream(), &original).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    assert_eq!(balance(&executor).await, 200);
    let simulated = executor
        .simulate(&Increment, metadata("increment"), &())
        .await
        .unwrap();
    assert_eq!(
        simulated
            .participants()
            .first()
            .unwrap()
            .events()
            .first()
            .unwrap()
            .payload(),
        3_i64.to_le_bytes()
    );
    assert_eq!(store.load(&stream()).await.unwrap(), original);
    executor
        .execute(&Increment, metadata("increment"), &())
        .await
        .unwrap();
    let history = store.load(&stream()).await.unwrap();
    assert_eq!(history.first(), original.first());
    codecs.validate_history(&stream(), &history).unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut dispatcher = DomainEventDispatcher::with_codecs(codecs.clone());
    assert_eq!(
        dispatcher.register_with_codec::<Counter, Added, _, _>(
            "override",
            Arc::new(JsonEventCodec),
            Arc::new(Observe(observed.clone()))
        ),
        Err(DomainEventRegistrationError::SharedCodecOverride)
    );
    dispatcher
        .register::<Counter, Added, _>("added", Arc::new(Observe(observed.clone())))
        .unwrap();
    for event in &history {
        dispatcher.dispatch(event).await.unwrap();
    }
    assert_eq!(*observed.lock().unwrap(), [200, 3]);
    let OutcomeLookup::Completed(retained) = store
        .lookup_outcome(&key(&store, "increment"))
        .await
        .unwrap()
    else {
        panic!("missing")
    };
    store
        .expire_details(&key(&store, "increment"), retained.marker().revision())
        .await
        .unwrap();
    assert_eq!(balance(&executor).await, 203);
    assert_eq!(
        store
            .load_transaction_receipt_in_context(
                metadata("increment").bounded_context().unwrap(),
                metadata("increment").operation_id()
            )
            .await
            .unwrap()
            .unwrap()
            .events()
            .len(),
        1
    );
    dispatcher.dispatch(history.last().unwrap()).await.unwrap();
    // Explicit replacement changes only this new configuration; shared consumers keep theirs.
    let mut replacement = (*codecs).clone();
    replacement
        .replace::<Counter, _>(ScaledCodec { scale: 1, calls })
        .unwrap();
    assert_eq!(
        balance(&CommandExecutor::with_codecs(store, Arc::new(replacement))).await,
        5
    );
    assert_eq!(balance(&executor).await, 203);
}

#[tokio::test]
async fn erased_validation_reports_the_original_malformed_event_without_json_fallback() {
    let mut codecs = AggregateCodecs::default();
    codecs
        .register::<Counter, _>(ScaledCodec {
            scale: 100,
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .unwrap();
    let operation = OperationId::new("bad").unwrap();
    let commit = derive_commit_id(&stream(), &operation);
    let event = NewEvent::new(derive_event_id(&commit, 0), "added", 2, b"2".to_vec()).unwrap();
    let batch = EventBatch::new(
        commit,
        operation,
        ContentFingerprint::digest("bad"),
        vec![event.clone()],
    )
    .unwrap();
    let recorded = InMemoryEventStore::new()
        .append(&stream(), ExpectedVersion::NoStream, batch)
        .await
        .unwrap()
        .into_events();
    assert!(
        matches!(codecs.validate_history(&stream(), &recorded), Err(CodecValidationError::Event { schema_version: 2, source, .. }) if source.kind() == EventCodecErrorKind::MalformedPayload)
    );
}

struct FixedReader(Result<DetailRead, DetailReadError>);
#[async_trait]
impl OutcomeDetailReader for FixedReader {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.0.clone()
    }
}

#[tokio::test]
async fn full_result_errors_keep_missing_outage_archive_and_expiry_distinct() {
    let store = InMemoryEventStore::new();
    let handler = Refuse(AtomicUsize::new(0));
    let codec = JsonRejectionCodec::new("string", 1);
    CommandExecutor::new(store.clone())
        .execute_with_rejection_codec(&handler, metadata("availability"), &(), &codec)
        .await
        .unwrap();
    let archive = ArchiveReference::new("independent:archive-pending").unwrap();
    for (read, expected) in [
        (
            Ok(DetailRead::AbsentHere),
            OutcomeRecoveryError::Unavailable {
                reason: DetailUnavailableReason::MissingDetails,
            },
        ),
        (
            Err(DetailReadError::Unavailable),
            OutcomeRecoveryError::Unavailable {
                reason: DetailUnavailableReason::BackendUnavailable,
            },
        ),
        (
            Ok(DetailRead::Archived {
                reference: archive.clone(),
            }),
            OutcomeRecoveryError::Archived { reference: archive },
        ),
    ] {
        let resolver = store
            .clone()
            .with_detail_readers(Arc::new(FixedReader(read)), None);
        assert_eq!(
            CommandExecutor::new(resolver)
                .execute_with_rejection_codec(&handler, metadata("availability"), &(), &codec)
                .await
                .unwrap_err(),
            CommandExecutionError::OutcomeRecovery(expected)
        );
    }
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
    let OutcomeLookup::Completed(retained) = store
        .lookup_outcome(&key(&store, "availability"))
        .await
        .unwrap()
    else {
        panic!("missing")
    };
    store
        .expire_details(&key(&store, "availability"), retained.marker().revision())
        .await
        .unwrap();
    let error = CommandExecutor::new(store)
        .execute_with_rejection_codec(&handler, metadata("availability"), &(), &codec)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        CommandExecutionError::OutcomeRecovery(OutcomeRecoveryError::Expired)
    );
    assert_eq!(
        error.execution_failure(),
        None,
        "deliberate expiry is not corrupted history"
    );
}
