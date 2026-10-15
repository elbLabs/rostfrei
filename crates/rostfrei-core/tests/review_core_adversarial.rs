//! Independent D1 review: expected guarantees, including deliberately RED regressions.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]

use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn metadata(id: &str) -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new(id).unwrap(),
        ContentFingerprint::digest(id),
    )
    .with_bounded_context(BoundedContextName::new("review").unwrap())
    .with_correlation_id(CorrelationId::new(id).unwrap())
    .with_security_scope(SecurityScope::new("scope-a").unwrap())
    .with_default_outcome_evidence::<()>()
}

fn proposal(id: &str, rejection: Option<&str>) -> EventTransaction {
    let m = metadata(id);
    EventTransaction::new(m.operation_id().clone(), m.operation_fingerprint(), vec![])
        .with_bounded_context(m.bounded_context().unwrap().clone())
        .with_correlation_id(m.correlation_id().unwrap().clone())
        .with_security_scope(m.security_scope().unwrap().clone())
        .with_completion(CommandCompletion::new(
            m.outcome_evidence().cloned(),
            rejection.map(|s| EncodedRejection {
                codec_id: "review".into(),
                codec_version: 1,
                bytes: serde_json::to_vec(s).unwrap(),
            }),
        ))
}

fn receipt(p: &EventTransaction, streams: Vec<TransactionStreamReceipt>) -> TransactionReceipt {
    let mut r =
        TransactionReceipt::new(p.operation_id().clone(), p.operation_fingerprint(), streams)
            .with_bounded_context(p.bounded_context().unwrap().clone());
    if let Some(c) = p.correlation_id() {
        r = r.with_correlation_id(c.clone());
    }
    if let Some(c) = p.causation_id() {
        r = r.with_causation_id(c.clone());
    }
    if let Some(s) = p.security_scope() {
        r = r.with_security_scope(s.clone());
    }
    r
}

async fn commit(store: &InMemoryEventStore, p: EventTransaction) -> CompletedOutcome {
    store
        .begin_command()
        .await
        .unwrap()
        .commit_outcome(p)
        .await
        .unwrap()
        .outcome()
        .clone()
}

#[allow(
    clippy::panic,
    reason = "missing control bytes must fail the independent fixture"
)]
async fn bytes(store: &InMemoryEventStore, outcome: &CompletedOutcome) -> Vec<u8> {
    let DetailRead::Found(bytes) = store
        .detail_reader()
        .get(outcome.marker().detail_reference())
        .await
        .unwrap()
    else {
        panic!("missing control bytes")
    };
    bytes
}

struct NeverExecute(AtomicUsize);
#[async_trait]
impl CommandHandler<()> for NeverExecute {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Rejected("unexpected reevaluation".into()))
    }
}

struct Reader {
    result: Result<DetailRead, DetailReadError>,
    calls: AtomicUsize,
}
impl Reader {
    fn new(result: Result<DetailRead, DetailReadError>) -> Arc<Self> {
        Arc::new(Self {
            result,
            calls: AtomicUsize::new(0),
        })
    }
}
#[async_trait]
impl OutcomeDetailReader for Reader {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.clone()
    }
}

#[tokio::test]
async fn cold_bytes_cannot_cross_operation_scope_or_authority_and_corrupt_hot_never_falls_back() {
    let store = InMemoryEventStore::new();
    let target = commit(&store, proposal("target", Some("winner"))).await;
    let own_bytes = bytes(&store, &target).await;
    let other = commit(&store, proposal("others", Some("winner"))).await;
    let foreign_store = InMemoryEventStore::new();
    let foreign = commit(&foreign_store, proposal("target", Some("winner"))).await;
    let (_, scoped_bytes) = OutcomeMarker::prepare(
        store.authority().clone(),
        &proposal("target", Some("winner"))
            .with_security_scope(SecurityScope::new("scope-b").unwrap()),
        receipt(
            &proposal("target", Some("winner"))
                .with_security_scope(SecurityScope::new("scope-b").unwrap()),
            vec![],
        ),
    )
    .unwrap();
    let mut flipped = own_bytes.clone();
    *flipped.last_mut().unwrap() ^= 1;
    for wrong in [
        bytes(&store, &other).await,
        bytes(&foreign_store, &foreign).await,
        scoped_bytes,
        flipped,
        own_bytes[..own_bytes.len() - 1].to_vec(),
    ] {
        let cold = Reader::new(Ok(DetailRead::Found(own_bytes.clone())));
        let hot = Reader::new(Ok(DetailRead::Found(wrong.clone())));
        let view = store.clone().with_detail_readers(hot, Some(cold.clone()));
        assert_eq!(
            view.lookup_outcome(target.marker().detail_reference().key())
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::CorruptHistory
        );
        assert_eq!(cold.calls.load(Ordering::SeqCst), 0);
        let cold = Reader::new(Ok(DetailRead::Found(wrong)));
        let view = store
            .clone()
            .with_detail_readers(Reader::new(Ok(DetailRead::AbsentHere)), Some(cold.clone()));
        let handler = NeverExecute(AtomicUsize::new(0));
        assert!(
            matches!(CommandExecutor::new(view).execute_with_rejection_codec(&handler, metadata("target"), &(), &JsonRejectionCodec::new("review", 1)).await,
            Err(CommandExecutionError::Store(e)) if e.kind() == EventStoreErrorKind::CorruptHistory)
        );
        assert_eq!(handler.0.load(Ordering::SeqCst), 0);
        assert_eq!(cold.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn forbidden_configuration_and_corrupt_reads_are_not_cold_misses() {
    let store = InMemoryEventStore::new();
    let target = commit(&store, proposal("target", Some("winner"))).await;
    for (fault, expected) in [
        (
            DetailReadError::Forbidden,
            EventStoreErrorKind::DetailForbidden,
        ),
        (
            DetailReadError::ConfigurationMismatch,
            EventStoreErrorKind::ConfigurationMismatch,
        ),
        (
            DetailReadError::Corrupt,
            EventStoreErrorKind::CorruptHistory,
        ),
    ] {
        let cold = Reader::new(Ok(DetailRead::Found(bytes(&store, &target).await)));
        let view = store
            .clone()
            .with_detail_readers(Reader::new(Err(fault)), Some(cold.clone()));
        assert_eq!(
            view.lookup_outcome(target.marker().detail_reference().key())
                .await
                .unwrap_err()
                .kind(),
            expected
        );
        assert_eq!(cold.calls.load(Ordering::SeqCst), 0);
    }
}

struct PausedReader {
    bytes: Vec<u8>,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    calls: AtomicUsize,
}
#[async_trait]
impl OutcomeDetailReader for PausedReader {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(DetailRead::Found(self.bytes.clone()))
    }
}

#[tokio::test]
async fn expiry_allows_only_the_already_admitted_read_and_never_caches_an_active_permit() {
    let store = InMemoryEventStore::new();
    let target = commit(&store, proposal("race", Some("secret"))).await;
    let paused = Arc::new(PausedReader {
        bytes: bytes(&store, &target).await,
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
    });
    let view = store.clone().with_detail_readers(paused.clone(), None);
    let key = target.marker().detail_reference().key().clone();
    let in_flight = view.lookup_outcome(&key);
    let expiry = async {
        paused.entered.notified().await;
        store
            .expire_details(&key, target.marker().revision())
            .await
            .unwrap();
        paused.release.notify_one();
    };
    let (admitted, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(in_flight, expiry)
    })
    .await
    .unwrap();
    let OutcomeLookup::Completed(admitted) = admitted.unwrap() else {
        panic!("missing")
    };
    assert!(matches!(admitted.details(), OutcomeDetails::Available(_)));
    for _ in 0..8 {
        let OutcomeLookup::Completed(withdrawn) = view.lookup_outcome(&key).await.unwrap() else {
            panic!("missing")
        };
        assert!(matches!(withdrawn.details(), OutcomeDetails::Expired));
        assert_eq!(withdrawn.marker(), target.marker());
    }
    assert_eq!(paused.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wrong_revision_is_write_free_and_concurrent_exact_replay_cannot_reopen_expiry() {
    let store = InMemoryEventStore::new();
    let p = proposal("winner", Some("secret"));
    let target = commit(&store, p.clone()).await;
    let other = commit(&store, proposal("other", Some("secret"))).await;
    let key = target.marker().detail_reference().key();
    let retained = bytes(&store, &target).await;
    assert_eq!(
        store.expire_details(key, other.marker().revision()).await,
        Err(OutcomeLifecycleError::RevisionMismatch)
    );
    assert_eq!(bytes(&store, &target).await, retained);
    let a = store.expire_details(key, target.marker().revision());
    let b = store.expire_details(key, target.marker().revision());
    let retry = async {
        store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(p.clone())
            .await
            .unwrap()
    };
    let (a, b, retry) = tokio::join!(a, b, retry);
    assert_eq!(a.unwrap(), b.unwrap());
    assert!(retry.is_exact_replay());
    assert!(matches!(retry.outcome().details(), OutcomeDetails::Expired));
    assert!(matches!(
        store
            .detail_reader()
            .get(target.marker().detail_reference())
            .await
            .unwrap(),
        DetailRead::AbsentHere
    ));
    assert!(
        matches!(store.begin_command().await.unwrap().commit_outcome(proposal("winner", Some("changed"))).await,
        Err(e) if e.kind() == EventStoreErrorKind::IdentityConflict)
    );
    assert_eq!(
        bytes(&store, &other).await.len(),
        other.marker().detail_reference().length()
    );
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("review-item").unwrap(),
        AggregateId::new(id).unwrap(),
    )
}

fn with_participants(
    p: &EventTransaction,
    participants: Vec<TransactionParticipant>,
) -> EventTransaction {
    let mut rebuilt = EventTransaction::new(
        p.operation_id().clone(),
        p.operation_fingerprint(),
        participants,
    )
    .with_bounded_context(p.bounded_context().unwrap().clone());
    if let Some(c) = p.correlation_id() {
        rebuilt = rebuilt.with_correlation_id(c.clone());
    }
    if let Some(c) = p.causation_id() {
        rebuilt = rebuilt.with_causation_id(c.clone());
    }
    if let Some(s) = p.security_scope() {
        rebuilt = rebuilt.with_security_scope(s.clone());
    }
    if let Some(c) = p.completion() {
        rebuilt = rebuilt.with_completion(c.clone());
    }
    rebuilt
}

fn batch(p: &EventTransaction, stream: &StreamId, payload: &[u8]) -> EventBatch {
    let commit = derive_commit_id(stream, p.operation_id());
    let event = NewEvent::new(derive_event_id(&commit, 0), "added", 1, payload.to_vec()).unwrap();
    let mut batch = EventBatch::new(
        commit,
        p.operation_id().clone(),
        p.operation_fingerprint(),
        vec![event],
    )
    .unwrap();
    if let Some(c) = p.correlation_id() {
        batch = batch.with_correlation_id(c.clone());
    }
    if let Some(c) = p.causation_id() {
        batch = batch.with_causation_id(c.clone());
    }
    batch
}

#[test]
fn proposal_digest_matches_the_original_exact_equality_over_adversarial_variants() {
    let p = proposal("digest", None);
    let write = TransactionParticipant::new(
        stream("write"),
        ExpectedVersion::NoStream,
        Some(batch(&p, &stream("write"), b"1")),
    );
    let guard = TransactionParticipant::new(stream("guard"), ExpectedVersion::NoStream, None);
    let p = with_participants(&p, vec![write.clone(), guard.clone()]);
    let mut completion = p.completion().unwrap().clone();
    completion.evidence.as_mut().unwrap().created_at_millis = Some(1);
    let mut evidence = p.completion().unwrap().clone();
    evidence.evidence.as_mut().unwrap().original_causation = Some("original".into());
    let variants = vec![
        p.clone(),
        with_participants(
            &p,
            vec![
                TransactionParticipant::new(
                    stream("write"),
                    ExpectedVersion::Exact(StreamVersion::new(9)),
                    write.batch().cloned(),
                ),
                guard.clone(),
            ],
        ),
        with_participants(&p, vec![guard.clone(), write.clone()]),
        with_participants(&p, vec![write]),
        with_participants(
            &p,
            vec![
                TransactionParticipant::new(
                    stream("write"),
                    ExpectedVersion::NoStream,
                    Some(batch(&p, &stream("write"), b"2")),
                ),
                guard,
            ],
        ),
        p.clone().with_completion(completion),
        p.clone().with_completion(evidence),
        p.clone()
            .with_security_scope(SecurityScope::new("scope-b").unwrap()),
        p.clone()
            .with_causation_id(CausationId::new("cause").unwrap()),
        p.clone()
            .with_bounded_context(BoundedContextName::new("other").unwrap()),
        p.with_correlation_id(CorrelationId::new("different").unwrap()),
        proposal("digest", Some("a:b")),
        proposal("digest", Some("ab:")),
    ];
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            let original_equal = a.bounded_context() == b.bounded_context()
                && a.operation_id() == b.operation_id()
                && a.operation_fingerprint() == b.operation_fingerprint()
                && a.correlation_id() == b.correlation_id()
                && a.causation_id() == b.causation_id()
                && a.security_scope() == b.security_scope()
                && a.completion() == b.completion()
                && a.participants().len() == b.participants().len()
                && a.participants()
                    .iter()
                    .zip(b.participants())
                    .all(|(x, y)| x.stream_id() == y.stream_id() && x.batch() == y.batch());
            assert_eq!(
                ProposalDigest::of(a).unwrap() == ProposalDigest::of(b).unwrap(),
                original_equal,
                "pair {i}/{j}"
            );
        }
    }
}

// Native-facing constructor regression: same top-level metadata is insufficient.
#[tokio::test]
async fn marker_prepare_must_reject_a_receipt_that_omits_proposed_events() {
    let store = InMemoryEventStore::new();
    let p = proposal("events", None);
    let p = with_participants(
        &p,
        vec![TransactionParticipant::new(
            stream("one"),
            ExpectedVersion::NoStream,
            Some(batch(&p, &stream("one"), b"1")),
        )],
    );
    let real = commit(&store, p.clone()).await;
    let wrong = OutcomeMarker::prepare(store.authority().clone(), &p, receipt(&p, vec![]));
    if let Ok((marker, _)) = &wrong {
        assert_eq!(
            marker.revision(),
            real.marker().revision(),
            "demonstrate identical revision despite contradictory integrity"
        );
        assert_ne!(
            marker.integrity_receipt(),
            real.marker().integrity_receipt()
        );
    }
    assert!(
        wrong.is_err(),
        "constructor accepted a receipt with zero streams/events for a one-event proposal"
    );
}

#[test]
fn marker_prepare_must_reject_a_receipt_with_the_wrong_guard_revision() {
    let p = with_participants(
        &proposal("guarded", Some("no")),
        vec![TransactionParticipant::new(
            stream("guard"),
            ExpectedVersion::Exact(StreamVersion::new(7)),
            None,
        )],
    );
    let wrong = receipt(
        &p,
        vec![TransactionStreamReceipt::new(
            stream("guard"),
            StreamVersion::new(8),
            vec![],
        )],
    );
    assert!(
        OutcomeMarker::prepare(CommandAuthority::new("review-native").unwrap(), &p, wrong).is_err(),
        "constructor accepted guard revision 8 for expected 7"
    );
}

/// Injects a read-result mixup; no malicious command data or source mutation is needed.
struct ReadResultBackend {
    authority: CommandAuthority,
    outcome: Result<CompletedOutcome, OutcomeRecoveryError>,
}
#[async_trait]
#[allow(clippy::panic, reason = "forbidden aggregate reads are test failures")]
impl EventHistory for ReadResultBackend {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("completed lookup must not read aggregates")
    }
}
#[async_trait]
impl OutcomeReader for ReadResultBackend {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        assert_eq!(key.authority(), &self.authority);
        self.outcome
            .clone()
            .map(OutcomeLookup::Completed)
            .map_err(|error| {
                EventStoreError::new(EventStoreErrorKind::CorruptHistory, error.to_string())
            })
    }
}
#[async_trait]
impl CommandBackend for ReadResultBackend {
    fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    #[allow(clippy::panic, reason = "completed replay must not begin an attempt")]
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        panic!("completed lookup must not start a session")
    }
}

#[tokio::test]
async fn completed_pair_must_reject_details_decoded_for_another_reference_and_decision() {
    let store = InMemoryEventStore::new();
    let rejected = commit(&store, proposal("rejected", Some("permanent denial"))).await;
    let accepted = commit(&store, proposal("accepted", None)).await;
    let spliced = CompletedOutcome::new(rejected.marker().clone(), accepted.details().clone());
    assert_eq!(rejected.marker().decision(), OutcomeDecision::Rejected);
    assert_eq!(
        spliced.as_ref().unwrap_err(),
        &OutcomeRecoveryError::IntegrityMismatch
    );
    let handler = NeverExecute(AtomicUsize::new(0));
    let result = CommandExecutor::new(ReadResultBackend {
        authority: store.authority().clone(),
        outcome: spliced,
    })
    .execute_with_rejection_codec(
        &handler,
        metadata("rejected"),
        &(),
        &JsonRejectionCodec::new("review", 1),
    )
    .await;
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    assert!(
        result.is_err(),
        "a Rejected marker returned business acceptance: {result:?}"
    );
}

#[tokio::test]
async fn completed_pair_must_reject_another_rejections_bytes_even_with_same_decision() {
    let store = InMemoryEventStore::new();
    let first = commit(&store, proposal("first", Some("original secret"))).await;
    let second = commit(&store, proposal("second", Some("other secret"))).await;
    let spliced = CompletedOutcome::new(first.marker().clone(), second.details().clone());
    assert_eq!(
        spliced.as_ref().unwrap_err(),
        &OutcomeRecoveryError::IntegrityMismatch
    );
    let result = CommandExecutor::new(ReadResultBackend {
        authority: store.authority().clone(),
        outcome: spliced,
    })
    .execute_with_rejection_codec(
        &NeverExecute(AtomicUsize::new(0)),
        metadata("first"),
        &(),
        &JsonRejectionCodec::new("review", 1),
    )
    .await;
    assert!(
        result.is_err(),
        "another operation's rejection passed replay: {result:?}"
    );
}

#[tokio::test]
async fn executor_must_reject_a_completed_marker_from_a_different_backend_authority() {
    let local = InMemoryEventStore::new();
    let foreign = InMemoryEventStore::new();
    let outcome = commit(&foreign, proposal("same", Some("foreign secret"))).await;
    assert_ne!(
        local.authority(),
        outcome.marker().detail_reference().key().authority()
    );
    let result = CommandExecutor::new(ReadResultBackend {
        authority: local.authority().clone(),
        outcome: Ok(outcome),
    })
    .execute_with_rejection_codec(
        &NeverExecute(AtomicUsize::new(0)),
        metadata("same"),
        &(),
        &JsonRejectionCodec::new("review", 1),
    )
    .await;
    assert!(result.is_err(), "foreign authority admitted: {result:?}");
}

struct Model;
struct Added(i64);
impl Aggregate for Model {
    type State = i64;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "review";
    const AGGREGATE_TYPE: &'static str = "review-item";
    fn initial(_: &StreamId) -> i64 {
        0
    }
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "bounded independent fixture values"
    )]
    fn apply(state: &mut i64, event: &Added) {
        *state += event.0;
    }
}
impl Event for Added {
    fn event_type(&self) -> &'static str {
        "added"
    }
    fn schema_version(&self) -> u32 {
        1
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
struct ScalingCodec(i64);
impl EventCodec<Model> for ScalingCodec {
    fn encode(&self, event: &Added, id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(id, "added", 1, event.0.to_string().into_bytes())
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string()))
    }
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "bounded independent fixture values"
    )]
    fn decode(&self, event: &RecordedEvent) -> Result<Added, EventCodecError> {
        Added::decode_json(event).map(|v| Added(v.0 * self.0))
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

#[tokio::test]
async fn duplicate_registry_failure_preserves_shared_codec_and_explicit_replacement_is_coherent() {
    let store = InMemoryEventStore::new();
    let p = proposal("history", None);
    let history = store
        .append(
            &stream("one"),
            ExpectedVersion::NoStream,
            batch(&p, &stream("one"), b"2"),
        )
        .await
        .unwrap()
        .into_events();
    let mut registry = AggregateCodecs::default();
    // An implicit default lookup is not an explicit reservation.
    registry.resolve::<Model>().unwrap();
    registry.register::<Model, _>(ScalingCodec(100)).unwrap();
    assert!(matches!(
        registry.register::<Model, _>(ScalingCodec(1)),
        Err(CodecRegistrationError::Duplicate { .. })
    ));
    assert_eq!(
        registry
            .resolve::<Model>()
            .unwrap()
            .decode(&history[0])
            .unwrap()
            .0,
        200
    );
    registry.replace::<Model, _>(ScalingCodec(10)).unwrap();
    let registry = Arc::new(registry);
    let observed = Arc::new(Mutex::new(vec![]));
    let mut dispatch = DomainEventDispatcher::with_codecs(registry.clone());
    dispatch
        .register::<Model, Added, _>("added", Arc::new(Observe(observed.clone())))
        .unwrap();
    dispatch.dispatch(&history[0]).await.unwrap();
    registry.validate_history(&stream("one"), &history).unwrap();
    assert_eq!(
        *CommandExecutor::with_codecs(store, registry)
            .rehydrate::<Model>(&stream("one"))
            .await
            .unwrap()
            .state(),
        20
    );
    assert_eq!(*observed.lock().unwrap(), [20]);
}

#[derive(Clone, Copy)]
enum CommitFault {
    RealConflictOnce,
    Unavailable,
    ExpiredOppositeWinner,
}

struct SessionBackend {
    inner: InMemoryEventStore,
    active: Arc<AtomicUsize>,
    begins: AtomicUsize,
    loads: AtomicUsize,
    commits: AtomicUsize,
    fault: CommitFault,
}
#[async_trait]
#[allow(
    clippy::panic,
    reason = "bypassing the consuming session is a test failure"
)]
impl EventHistory for SessionBackend {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("execution must read through its consuming session")
    }
}
#[async_trait]
impl OutcomeReader for SessionBackend {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        assert_eq!(
            self.active.load(Ordering::SeqCst),
            0,
            "reconciliation before consuming the attempt"
        );
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl CommandBackend for SessionBackend {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        assert_eq!(self.active.fetch_add(1, Ordering::SeqCst), 0);
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(TracedSession {
            backend: self,
            inner: Some(self.inner.begin_command().await?),
        }))
    }
}
struct TracedSession<'a> {
    backend: &'a SessionBackend,
    inner: Option<Box<dyn CommandCommitSession + 'a>>,
}
impl Drop for TracedSession<'_> {
    fn drop(&mut self) {
        assert_eq!(self.backend.active.fetch_sub(1, Ordering::SeqCst), 1);
    }
}
#[async_trait]
impl EventHistory for TracedSession<'_> {
    async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.backend.loads.fetch_add(1, Ordering::SeqCst);
        self.inner.as_ref().unwrap().load(stream).await
    }
}
#[async_trait]
impl CommandCommitSession for TracedSession<'_> {
    async fn commit_outcome(
        mut self: Box<Self>,
        p: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        let attempt = self.backend.commits.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            p.participants().len(),
            2,
            "both read guards must survive rejection finalization"
        );
        assert!(p.participants().iter().all(|p| p.batch().is_none()));
        match self.backend.fault {
            CommitFault::RealConflictOnce if attempt == 0 => {
                let rival = proposal("rival", None);
                self.backend
                    .inner
                    .append(
                        &stream("guard"),
                        ExpectedVersion::NoStream,
                        batch(&rival, &stream("guard"), b"3"),
                    )
                    .await?;
            }
            CommitFault::Unavailable => {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::Unavailable,
                    "session authority uncertain",
                ));
            }
            CommitFault::ExpiredOppositeWinner => {
                let accepted = with_participants(&p, vec![]).with_completion(
                    CommandCompletion::new(p.completion().unwrap().evidence.clone(), None),
                );
                let winner = commit(&self.backend.inner, accepted).await;
                self.backend
                    .inner
                    .expire_details(
                        winner.marker().detail_reference().key(),
                        winner.marker().revision(),
                    )
                    .await
                    .unwrap();
            }
            CommitFault::RealConflictOnce => {}
        }
        self.inner.take().unwrap().commit_outcome(p).await
    }
}

struct GuardedRefusal(Arc<Mutex<Vec<i64>>>);
#[async_trait]
impl CommandHandler<()> for GuardedRefusal {
    type Rejection = String;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let guard = execution.load::<Model>("guard").await?;
        let mut discarded = execution.load::<Model>("discarded").await?;
        self.0.lock().unwrap().push(*guard.state());
        discarded.raise(Added(9));
        Ok(CommandDecision::Rejected(format!(
            "observed:{}",
            guard.state()
        )))
    }
}
struct SessionPolicy {
    active: Arc<AtomicUsize>,
    calls: AtomicUsize,
    decision: ConflictRetryDecision,
}
impl ConflictRetryPolicy for SessionPolicy {
    fn after_conflict(&self, retry: ConflictRetry) -> ConflictRetryDecision {
        assert_eq!(self.active.load(Ordering::SeqCst), 0);
        assert_eq!(retry.error().kind(), EventStoreErrorKind::Conflict);
        assert_eq!(retry.evaluations(), 1);
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.decision
    }
}

#[tokio::test]
async fn consumed_sessions_preserve_rejection_guards_and_policy_sees_only_unresolved_conflicts() {
    for fault in [
        CommitFault::RealConflictOnce,
        CommitFault::Unavailable,
        CommitFault::ExpiredOppositeWinner,
    ] {
        let active = Arc::new(AtomicUsize::new(0));
        let store = SessionBackend {
            inner: InMemoryEventStore::new(),
            active: active.clone(),
            begins: AtomicUsize::new(0),
            loads: AtomicUsize::new(0),
            commits: AtomicUsize::new(0),
            fault,
        };
        let seen = Arc::new(Mutex::new(vec![]));
        let policy = Arc::new(SessionPolicy {
            active: active.clone(),
            calls: AtomicUsize::new(0),
            decision: ConflictRetryDecision::RetryAfter(Duration::from_millis(1)),
        });
        let executor = CommandExecutor::new(store).with_conflict_retry_policy(policy.clone());
        let result = executor
            .execute_with_rejection_codec(
                &GuardedRefusal(seen.clone()),
                metadata("session"),
                &(),
                &JsonRejectionCodec::new("review", 1),
            )
            .await;
        let attempts = match fault {
            CommitFault::RealConflictOnce => {
                assert_eq!(
                    result.unwrap(),
                    CommandOutcome::Rejected("observed:3".into())
                );
                assert_eq!(*seen.lock().unwrap(), [0, 3]);
                assert_eq!(policy.calls.load(Ordering::SeqCst), 1);
                2
            }
            CommitFault::Unavailable => {
                assert!(
                    matches!(result, Err(CommandExecutionError::RejectedOutcomePersistence(e)) if e.kind() == EventStoreErrorKind::Unavailable)
                );
                assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
                1
            }
            CommitFault::ExpiredOppositeWinner => {
                assert_eq!(
                    result.unwrap_err(),
                    CommandExecutionError::OutcomeRecovery(OutcomeRecoveryError::Expired)
                );
                assert_eq!(policy.calls.load(Ordering::SeqCst), 0);
                1
            }
        };
        assert_eq!(executor.store().begins.load(Ordering::SeqCst), attempts);
        assert_eq!(executor.store().loads.load(Ordering::SeqCst), attempts * 2);
        assert_eq!(executor.store().commits.load(Ordering::SeqCst), attempts);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(
            executor
                .store()
                .inner
                .load(&stream("discarded"))
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn zero_budget_reconciles_a_withdrawn_opposite_winner_without_reevaluation() {
    let active = Arc::new(AtomicUsize::new(0));
    let store = SessionBackend {
        inner: InMemoryEventStore::new(),
        active: active.clone(),
        begins: AtomicUsize::new(0),
        loads: AtomicUsize::new(0),
        commits: AtomicUsize::new(0),
        fault: CommitFault::ExpiredOppositeWinner,
    };
    let executor =
        CommandExecutor::new(store).with_conflict_retry_policy(Arc::new(NoConflictRetries));
    let seen = Arc::new(Mutex::new(vec![]));
    let result = executor
        .execute_with_rejection_codec(
            &GuardedRefusal(seen.clone()),
            metadata("withdrawn-winner"),
            &(),
            &JsonRejectionCodec::new("review", 1),
        )
        .await;
    assert_eq!(
        result.unwrap_err(),
        CommandExecutionError::OutcomeRecovery(OutcomeRecoveryError::Expired)
    );
    assert_eq!(*seen.lock().unwrap(), [0]);
    assert_eq!(executor.store().begins.load(Ordering::SeqCst), 1);
    assert_eq!(active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unsupported_rejection_codec_blocks_completed_replay_without_session_or_handler() {
    let store = InMemoryEventStore::new();
    let outcome = commit(&store, proposal("unsupported", Some("winner"))).await;
    let handler = NeverExecute(AtomicUsize::new(0));
    let backend = ReadResultBackend {
        authority: store.authority().clone(),
        outcome: Ok(outcome),
    };
    assert!(
        matches!(CommandExecutor::new(backend).execute_with_rejection_codec(&handler, metadata("unsupported"), &(), &JsonRejectionCodec::new("review", 2)).await,
        Err(CommandExecutionError::Codec(e)) if e.kind() == EventCodecErrorKind::UnsupportedSchemaVersion)
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);
}

// Characterization, not a failure assertion: the convenience builder forks a supplied registry.
#[tokio::test]
async fn observation_with_codec_overrides_and_forks_a_shared_registry() {
    let store = InMemoryEventStore::new();
    let p = proposal("history", None);
    let history = store
        .append(
            &stream("one"),
            ExpectedVersion::NoStream,
            batch(&p, &stream("one"), b"2"),
        )
        .await
        .unwrap()
        .into_events();
    let mut registry = AggregateCodecs::default();
    registry.register::<Model, _>(ScalingCodec(100)).unwrap();
    let registry = Arc::new(registry);
    let executor = CommandExecutor::with_codecs(store, registry.clone())
        .with_codec::<Model, _>(ScalingCodec(1))
        .unwrap();
    let executor = executor.with_codec::<Model, _>(ScalingCodec(10)).unwrap();
    assert_eq!(
        *executor
            .rehydrate::<Model>(&stream("one"))
            .await
            .unwrap()
            .state(),
        20
    );
    assert_eq!(
        registry
            .resolve::<Model>()
            .unwrap()
            .decode(&history[0])
            .unwrap()
            .0,
        200
    );
}
