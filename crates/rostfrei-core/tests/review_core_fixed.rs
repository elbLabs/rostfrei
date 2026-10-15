//! Independent repair confirmation; additional cases beyond the imported D1 suite.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#![allow(
    clippy::as_conversions,
    clippy::panic,
    reason = "retained independent type-erasure and forbidden-path assertions"
)]
use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

fn metadata() -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new("operation").unwrap(),
        ContentFingerprint::digest("request"),
    )
    .with_bounded_context(BoundedContextName::new("review-fixed").unwrap())
    .with_correlation_id(CorrelationId::new("correlation").unwrap())
    .with_causation_id(CausationId::new("cause").unwrap())
    .with_security_scope(SecurityScope::new("scope-a").unwrap())
    .with_default_outcome_evidence::<()>()
}
fn proposal(
    m: &CommandExecutionMetadata,
    secret: Option<&str>,
    participants: Vec<TransactionParticipant>,
) -> EventTransaction {
    EventTransaction::new(
        m.operation_id().clone(),
        m.operation_fingerprint(),
        participants,
    )
    .with_bounded_context(m.bounded_context().unwrap().clone())
    .with_correlation_id(m.correlation_id().unwrap().clone())
    .with_causation_id(m.causation_id().unwrap().clone())
    .with_security_scope(m.security_scope().unwrap().clone())
    .with_completion(CommandCompletion::new(
        m.outcome_evidence().cloned(),
        secret.map(|s| {
            JsonRejectionCodec::new("review", 1)
                .encode(&s.to_owned())
                .unwrap()
        }),
    ))
}
fn receipt(p: &EventTransaction, streams: Vec<TransactionStreamReceipt>) -> TransactionReceipt {
    TransactionReceipt::new(p.operation_id().clone(), p.operation_fingerprint(), streams)
        .with_bounded_context(p.bounded_context().unwrap().clone())
        .with_correlation_id(p.correlation_id().unwrap().clone())
        .with_causation_id(p.causation_id().unwrap().clone())
        .with_security_scope(p.security_scope().unwrap().clone())
}
fn prepared(authority: &CommandAuthority, p: &EventTransaction) -> CompletedOutcome {
    let (marker, bytes) = OutcomeMarker::prepare(authority.clone(), p, receipt(p, vec![])).unwrap();
    let details = EncodedOutcomeDetails::decode(marker.detail_reference(), &bytes).unwrap();
    CompletedOutcome::new(marker, OutcomeDetails::Available(details)).unwrap()
}

#[test]
fn nested_type_erasure_cannot_restore_foreign_encoded_authority() {
    let authority = CommandAuthority::new("local").unwrap();
    let p = proposal(&metadata(), Some("first"), vec![]);
    let target = prepared(&authority, &p);
    let mut timestamp = p.completion().unwrap().clone();
    timestamp.evidence.as_mut().unwrap().created_at_millis = Some(1);
    let mut codec = p.completion().unwrap().clone();
    codec.rejection.as_mut().unwrap().codec_version = 2;
    for foreign in [
        prepared(&authority, &proposal(&metadata(), Some("other"), vec![])),
        prepared(&authority, &p.clone().with_completion(timestamp)),
        prepared(&authority, &p.clone().with_completion(codec)),
        prepared(
            &authority,
            &proposal(
                &metadata().with_security_scope(SecurityScope::new("scope-b").unwrap()),
                Some("first"),
                vec![],
            ),
        ),
        prepared(&CommandAuthority::new("other").unwrap(), &p),
    ] {
        let detail = foreign.details().available().unwrap().clone();
        let erased: CompletedOutcome<Box<dyn Any>> = target
            .clone()
            .try_map::<_, OutcomeRecoveryError>(|_| Ok(Box::new(detail.clone()) as Box<dyn Any>))
            .unwrap();
        let restored = erased.try_map::<_, OutcomeRecoveryError>(|v| {
            Ok(*v.downcast::<EncodedOutcomeDetails>().unwrap())
        });
        assert_eq!(
            restored.unwrap_err(),
            OutcomeRecoveryError::IntegrityMismatch
        );
        let wrapped = CompletedOutcome::new(
            target.marker().clone(),
            OutcomeDetails::Available(Arc::new(Some(detail))),
        )
        .unwrap();
        let restored = wrapped
            .try_map::<_, OutcomeRecoveryError>(|v| Ok(v.as_ref().as_ref().unwrap().clone()));
        assert_eq!(
            restored.unwrap_err(),
            OutcomeRecoveryError::IntegrityMismatch
        );
    }
    // The same reference through the same type-erasure route is a legitimate projection.
    let original = target.to_inline().unwrap();
    let erased: CompletedOutcome<Box<dyn Any>> = target
        .try_map::<_, OutcomeRecoveryError>(|v| Ok(Box::new(v) as Box<dyn Any>))
        .unwrap();
    let restored = erased
        .try_map::<_, OutcomeRecoveryError>(|v| Ok(*v.downcast::<EncodedOutcomeDetails>().unwrap()))
        .unwrap();
    assert_eq!(restored.to_inline().unwrap(), original);
}

fn unavailable_states() -> Vec<OutcomeDetails<EncodedOutcomeDetails>> {
    vec![
        OutcomeDetails::Expired,
        OutcomeDetails::Archived {
            reference: ArchiveReference::new("archive:review").unwrap(),
        },
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::MissingDetails,
        },
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::BackendUnavailable,
        },
    ]
}

#[test]
fn mapping_unavailable_states_never_invokes_a_projection_or_restores_payload() {
    let target = prepared(
        &CommandAuthority::new("local").unwrap(),
        &proposal(&metadata(), Some("secret"), vec![]),
    );
    for state in unavailable_states() {
        let unavailable = CompletedOutcome::new(target.marker().clone(), state.clone()).unwrap();
        let mapped = unavailable
            .try_map::<String, OutcomeRecoveryError>(|_| {
                panic!("unavailable state invoked application projection")
            })
            .unwrap();
        let restored = mapped
            .try_map::<EncodedOutcomeDetails, OutcomeRecoveryError>(|_| {
                panic!("unavailable state invoked encoded projection")
            })
            .unwrap();
        assert_eq!(restored.details(), &state);
        assert_eq!(restored.marker(), target.marker());
        assert!(restored.to_inline().is_err());
    }
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("item").unwrap(),
        AggregateId::new(id).unwrap(),
    )
}
fn batch(fault: &str) -> EventBatch {
    let m = metadata();
    let commit = derive_commit_id(&stream("write"), m.operation_id());
    let event = NewEvent::new(derive_event_id(&commit, 0), "added", 1, b"value".to_vec()).unwrap();
    EventBatch::new(
        if fault == "commit" {
            CommitId::new("other").unwrap()
        } else {
            commit
        },
        if fault == "operation" {
            OperationId::new("other").unwrap()
        } else {
            m.operation_id().clone()
        },
        if fault == "fingerprint" {
            ContentFingerprint::digest("other")
        } else {
            m.operation_fingerprint()
        },
        vec![event],
    )
    .unwrap()
    .with_correlation_id(if fault == "correlation" {
        CorrelationId::new("other").unwrap()
    } else {
        m.correlation_id().unwrap().clone()
    })
    .with_causation_id(if fault == "causation" {
        CausationId::new("other").unwrap()
    } else {
        m.causation_id().unwrap().clone()
    })
}

#[tokio::test]
async fn proposal_batch_metadata_and_completion_must_match_an_internally_valid_receipt() {
    let store = InMemoryEventStore::new();
    let m = metadata();
    let p = proposal(
        &m,
        None,
        vec![TransactionParticipant::new(
            stream("write"),
            ExpectedVersion::NoStream,
            Some(batch("")),
        )],
    );
    let actual = store
        .begin_command()
        .await
        .unwrap()
        .commit_outcome(p.clone())
        .await
        .unwrap();
    let r = actual.outcome().marker().integrity_receipt();
    r.validate_integrity().unwrap();
    for fault in [
        "commit",
        "operation",
        "fingerprint",
        "correlation",
        "causation",
    ] {
        let changed = proposal(
            &m,
            None,
            vec![TransactionParticipant::new(
                stream("write"),
                ExpectedVersion::NoStream,
                Some(batch(fault)),
            )],
        );
        assert_eq!(
            r.validate_proposal(&changed).unwrap_err().kind(),
            EventStoreErrorKind::InvalidRequest,
            "{fault}"
        );
    }
    for changed in [
        p.clone()
            .with_security_scope(SecurityScope::new("other").unwrap()),
        p.clone()
            .with_bounded_context(BoundedContextName::new("other").unwrap()),
        p.clone()
            .with_causation_id(CausationId::new("other").unwrap()),
        p.clone()
            .with_correlation_id(CorrelationId::new("other").unwrap()),
        proposal(
            &m,
            None,
            vec![TransactionParticipant::new(
                stream("write"),
                ExpectedVersion::Exact(StreamVersion::ZERO),
                Some(batch("")),
            )],
        ),
        proposal(
            &m,
            None,
            vec![TransactionParticipant::new(
                stream("write"),
                ExpectedVersion::NoStream,
                None,
            )],
        ),
    ] {
        assert!(r.validate_proposal(&changed).is_err());
    }
    let mut different = p.completion().unwrap().clone();
    different.evidence.as_mut().unwrap().created_at_millis = Some(1);
    assert!(
        r.clone()
            .with_completion(different)
            .validate_proposal(&p)
            .is_err()
    );
    r.clone()
        .with_completion(p.completion().unwrap().clone())
        .validate_proposal(&p)
        .unwrap();
}

#[tokio::test]
async fn stronger_prepare_validation_preserves_exact_replay_after_guard_versions_change_and_expiry()
{
    for secret in [None, Some("no")] {
        let store = InMemoryEventStore::new();
        let p = proposal(
            &metadata(),
            secret,
            vec![TransactionParticipant::new(
                stream("guard"),
                ExpectedVersion::NoStream,
                None,
            )],
        );
        let first = store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(p)
            .await
            .unwrap();
        let changed = proposal(
            &metadata(),
            secret,
            vec![TransactionParticipant::new(
                stream("guard"),
                ExpectedVersion::Exact(StreamVersion::new(99)),
                None,
            )],
        );
        assert!(
            first
                .outcome()
                .marker()
                .integrity_receipt()
                .validate_proposal(&changed)
                .is_err()
        );
        assert_eq!(
            first.outcome().marker().proposal_digest(),
            ProposalDigest::of(&changed).unwrap()
        );
        let replay = store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(changed.clone())
            .await
            .unwrap();
        assert!(replay.is_exact_replay());
        assert_eq!(first.outcome(), replay.outcome());
        let marker = first.outcome().marker();
        store
            .expire_details(marker.detail_reference().key(), marker.revision())
            .await
            .unwrap();
        let replay = store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(changed)
            .await
            .unwrap();
        assert!(replay.is_exact_replay());
        assert_eq!(marker, replay.outcome().marker());
        assert!(matches!(
            replay.outcome().details(),
            OutcomeDetails::Expired
        ));
    }
}

#[derive(Clone, Copy, Debug)]
enum ReturnPath {
    Initial,
    Appended,
    ExactReplay,
    Reconcile,
}
struct Backend {
    authority: CommandAuthority,
    outcome: CompletedOutcome,
    path: ReturnPath,
    committed: AtomicBool,
    begins: AtomicUsize,
}
#[async_trait]
impl EventHistory for Backend {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("unexpected history")
    }
}
#[async_trait]
impl OutcomeReader for Backend {
    async fn lookup_outcome(
        &self,
        _: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        Ok(
            if matches!(self.path, ReturnPath::Initial) || self.committed.load(Ordering::SeqCst) {
                OutcomeLookup::Completed(self.outcome.clone())
            } else {
                OutcomeLookup::Missing
            },
        )
    }
}
struct Session<'a>(&'a Backend);
#[async_trait]
impl EventHistory for Session<'_> {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("unexpected session history")
    }
}
#[async_trait]
impl CommandCommitSession for Session<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        _: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.0.committed.store(true, Ordering::SeqCst);
        match self.0.path {
            ReturnPath::Appended => Ok(CompletedCommit::Appended(self.0.outcome.clone())),
            ReturnPath::ExactReplay => Ok(CompletedCommit::ExactReplay(self.0.outcome.clone())),
            ReturnPath::Reconcile => Err(EventStoreError::new(
                EventStoreErrorKind::IdentityConflict,
                "opposite proposal winner",
            )),
            ReturnPath::Initial => panic!("completed lookup must not commit"),
        }
    }
}
#[async_trait]
impl CommandBackend for Backend {
    fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Session(self)))
    }
}
struct Accept(AtomicUsize);
#[async_trait]
impl CommandHandler<()> for Accept {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Accepted)
    }
}
async fn reject_all_paths(outcome: CompletedOutcome, kind: EventStoreErrorKind) {
    for path in [
        ReturnPath::Initial,
        ReturnPath::Appended,
        ReturnPath::ExactReplay,
        ReturnPath::Reconcile,
    ] {
        let backend = Arc::new(Backend {
            authority: CommandAuthority::new("local").unwrap(),
            outcome: outcome.clone(),
            path,
            committed: AtomicBool::new(false),
            begins: AtomicUsize::new(0),
        });
        let handler = Accept(AtomicUsize::new(0));
        let result = CommandExecutor::new(backend.clone())
            .with_max_conflict_retries(0)
            .execute_with_rejection_codec(
                &handler,
                metadata(),
                &(),
                &JsonRejectionCodec::new("review", 1),
            )
            .await;
        assert!(
            matches!(result, Err(CommandExecutionError::Store(ref e)) if e.kind()==kind),
            "{path:?}: {result:?}"
        );
        let expected = usize::from(!matches!(path, ReturnPath::Initial));
        assert_eq!(handler.0.load(Ordering::SeqCst), expected);
        assert_eq!(backend.begins.load(Ordering::SeqCst), expected);
        if matches!(path, ReturnPath::Initial) {
            let result = CommandExecutor::new(backend)
                .reconcile_with_rejection_codec::<String>(
                    metadata(),
                    &JsonRejectionCodec::new("review", 1),
                )
                .await;
            assert!(matches!(result, Err(CommandExecutionError::Store(e)) if e.kind()==kind));
        }
    }
}

#[tokio::test]
async fn same_key_identity_scope_changes_fail_on_every_return_path_including_identity_conflict_reconciliation()
 {
    let original = metadata();
    let mut changed_evidence = original.outcome_evidence().unwrap().clone();
    changed_evidence.command_schema = 2;
    let changed_fingerprint = CommandExecutionMetadata::new(
        original.operation_id().clone(),
        ContentFingerprint::digest("other"),
    )
    .with_bounded_context(original.bounded_context().unwrap().clone())
    .with_correlation_id(original.correlation_id().unwrap().clone())
    .with_causation_id(original.causation_id().unwrap().clone())
    .with_security_scope(original.security_scope().unwrap().clone())
    .with_outcome_evidence(original.outcome_evidence().unwrap().clone());
    for changed in [
        original
            .clone()
            .with_security_scope(SecurityScope::new("other").unwrap()),
        original
            .clone()
            .with_correlation_id(CorrelationId::new("other").unwrap()),
        original
            .clone()
            .with_causation_id(CausationId::new("other").unwrap()),
        original.with_outcome_evidence(changed_evidence),
        changed_fingerprint,
    ] {
        let outcome = prepared(
            &CommandAuthority::new("local").unwrap(),
            &proposal(&changed, Some("secret"), vec![]),
        );
        reject_all_paths(outcome, EventStoreErrorKind::IdentityConflict).await;
    }
}

#[tokio::test]
async fn unavailable_foreign_results_do_not_bypass_full_key_validation() {
    let foreign = prepared(
        &CommandAuthority::new("foreign").unwrap(),
        &proposal(&metadata(), Some("secret"), vec![]),
    );
    for state in unavailable_states() {
        let outcome = CompletedOutcome::new(foreign.marker().clone(), state).unwrap();
        reject_all_paths(outcome, EventStoreErrorKind::CorruptHistory).await;
    }
}
