//! Additional constructor, generic projection and consuming-result fault coverage.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

fn metadata() -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new("operation").unwrap(),
        ContentFingerprint::digest("request"),
    )
    .with_bounded_context(BoundedContextName::new("context").unwrap())
    .with_correlation_id(CorrelationId::new("correlation").unwrap())
    .with_causation_id(CausationId::new("cause").unwrap())
    .with_security_scope(SecurityScope::new("scope").unwrap())
    .with_default_outcome_evidence::<()>()
}
fn proposal(
    m: &CommandExecutionMetadata,
    secret: &str,
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
        Some(
            JsonRejectionCodec::new("secret", 1)
                .encode(&secret.to_owned())
                .unwrap(),
        ),
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
fn generic_projection_cannot_launder_foreign_encoded_details_even_for_the_same_operation() {
    let authority = CommandAuthority::new("authority").unwrap();
    let target = prepared(&authority, &proposal(&metadata(), "original", vec![]));
    for foreign in [
        prepared(&authority, &proposal(&metadata(), "different body", vec![])),
        prepared(
            &CommandAuthority::new("foreign").unwrap(),
            &proposal(&metadata(), "original", vec![]),
        ),
        prepared(
            &authority,
            &proposal(
                &metadata().with_security_scope(SecurityScope::new("other").unwrap()),
                "original",
                vec![],
            ),
        ),
    ] {
        let detail = foreign.details().available().unwrap().clone();
        assert_eq!(
            detail.detail_reference(),
            foreign.marker().detail_reference()
        );
        assert_ne!(
            detail.detail_reference(),
            target.marker().detail_reference()
        );
        let direct = target
            .clone()
            .try_map::<_, OutcomeRecoveryError>(|_| Ok(detail.clone()));
        assert_eq!(direct.unwrap_err(), OutcomeRecoveryError::IntegrityMismatch);
        let intermediate = target
            .clone()
            .try_map::<_, OutcomeRecoveryError>(|_| Ok("projected DTO".to_owned()))
            .unwrap();
        let indirect = intermediate.try_map::<_, OutcomeRecoveryError>(|_| Ok(detail));
        assert_eq!(
            indirect.unwrap_err(),
            OutcomeRecoveryError::IntegrityMismatch
        );
    }
    let original = target.to_inline().unwrap();
    let identity = target.try_map::<_, OutcomeRecoveryError>(Ok).unwrap();
    assert_eq!(identity.to_inline().unwrap(), original);
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new("model").unwrap(),
        AggregateId::new(id).unwrap(),
    )
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one coherent receipt control with independent field substitutions"
)]
fn preparation_checks_all_event_coordinates_contents_and_ordered_participants() {
    let m = metadata();
    let write = stream("write");
    let guard = stream("guard");
    let commit = derive_commit_id(&write, m.operation_id());
    let batch = EventBatch::new(
        commit.clone(),
        m.operation_id().clone(),
        m.operation_fingerprint(),
        (0..2)
            .map(|i| {
                NewEvent::new(
                    derive_event_id(&commit, i),
                    "added",
                    2,
                    vec![u8::try_from(i).unwrap()],
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
    .with_correlation_id(m.correlation_id().unwrap().clone())
    .with_causation_id(m.causation_id().unwrap().clone());
    let p = proposal(
        &m,
        "unused",
        vec![
            TransactionParticipant::new(
                write.clone(),
                ExpectedVersion::Exact(StreamVersion::new(7)),
                Some(batch),
            ),
            TransactionParticipant::new(guard.clone(), ExpectedVersion::NoStream, None),
        ],
    )
    .with_completion(CommandCompletion::new(m.outcome_evidence().cloned(), None));
    let event = |ordinal: u32, fault: &str| {
        RecordedEvent::new_in_commit(
            if fault == "stream" {
                guard.clone()
            } else {
                write.clone()
            },
            StreamVersion::new(if fault == "version" {
                50
            } else {
                8 + u64::from(ordinal)
            }),
            if fault == "event-id" {
                EventId::new("foreign").unwrap()
            } else {
                derive_event_id(&commit, ordinal)
            },
            if fault == "commit" {
                CommitId::new("foreign").unwrap()
            } else {
                commit.clone()
            },
            if fault == "operation" {
                OperationId::new("foreign").unwrap()
            } else {
                m.operation_id().clone()
            },
            if fault == "fingerprint" {
                ContentFingerprint::digest("foreign")
            } else {
                m.operation_fingerprint()
            },
            if fault == "ordinal" { 1 } else { ordinal },
            if fault == "count" { 3 } else { 2 },
            if fault == "type" { "other" } else { "added" },
            if fault == "schema" { 3 } else { 2 },
            if fault == "payload" {
                vec![99]
            } else {
                vec![u8::try_from(ordinal).unwrap()]
            },
        )
        .unwrap()
        .with_correlation_id(if fault == "correlation" {
            CorrelationId::new("foreign").unwrap()
        } else {
            m.correlation_id().unwrap().clone()
        })
        .with_causation_id(if fault == "causation" {
            CausationId::new("foreign").unwrap()
        } else {
            m.causation_id().unwrap().clone()
        })
    };
    let valid_stream = TransactionStreamReceipt::new(
        write.clone(),
        StreamVersion::new(7),
        vec![event(0, ""), event(1, "")],
    );
    let valid_guard = TransactionStreamReceipt::new(guard.clone(), StreamVersion::ZERO, vec![]);
    let valid = receipt(&p, vec![valid_stream.clone(), valid_guard.clone()]);
    valid.validate_proposal(&p).unwrap();
    let authority = CommandAuthority::new("receipt-validation").unwrap();
    OutcomeMarker::prepare(authority.clone(), &p, valid).unwrap();
    for fault in [
        "stream",
        "version",
        "event-id",
        "commit",
        "operation",
        "fingerprint",
        "ordinal",
        "count",
        "type",
        "schema",
        "payload",
        "correlation",
        "causation",
    ] {
        let wrong = receipt(
            &p,
            vec![
                TransactionStreamReceipt::new(
                    write.clone(),
                    StreamVersion::new(7),
                    vec![event(0, fault), event(1, "")],
                ),
                valid_guard.clone(),
            ],
        );
        assert_eq!(
            OutcomeMarker::prepare(authority.clone(), &p, wrong)
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::InvalidRequest,
            "{fault}"
        );
    }
    for streams in [
        vec![valid_guard.clone(), valid_stream.clone()],
        vec![valid_stream.clone()],
        vec![
            valid_stream.clone(),
            valid_guard.clone(),
            valid_guard.clone(),
        ],
        vec![valid_stream.clone(), valid_stream.clone()],
        vec![
            TransactionStreamReceipt::new(write.clone(), StreamVersion::new(7), vec![event(0, "")]),
            valid_guard.clone(),
        ],
        vec![
            TransactionStreamReceipt::new(
                write.clone(),
                StreamVersion::new(7),
                vec![event(1, ""), event(0, "")],
            ),
            valid_guard,
        ],
        vec![
            valid_stream,
            TransactionStreamReceipt::new(guard, StreamVersion::new(1), vec![]),
        ],
    ] {
        assert!(OutcomeMarker::prepare(authority.clone(), &p, receipt(&p, streams)).is_err());
    }
}

#[derive(Clone, Copy)]
enum Path {
    Lookup,
    Appended,
    ExactReplay,
    Conflict,
}
struct FaultBackend {
    authority: CommandAuthority,
    outcome: CompletedOutcome,
    path: Path,
    committed: AtomicBool,
    begins: AtomicUsize,
}
#[async_trait]
#[allow(
    clippy::panic,
    reason = "these event-free attempts must not read aggregates"
)]
impl EventHistory for FaultBackend {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("no history expected")
    }
}
#[async_trait]
impl OutcomeReader for FaultBackend {
    async fn lookup_outcome(
        &self,
        _: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        Ok(
            if matches!(self.path, Path::Lookup) || self.committed.load(Ordering::SeqCst) {
                OutcomeLookup::Completed(self.outcome.clone())
            } else {
                OutcomeLookup::Missing
            },
        )
    }
}
struct FaultSession<'a>(&'a FaultBackend);
#[async_trait]
#[allow(
    clippy::panic,
    reason = "these event-free attempts must not read aggregates"
)]
impl EventHistory for FaultSession<'_> {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        panic!("no history expected")
    }
}
#[async_trait]
#[allow(clippy::panic, reason = "completed lookup must not reach commit")]
impl CommandCommitSession for FaultSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        _: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.0.committed.store(true, Ordering::SeqCst);
        match self.0.path {
            Path::Appended => Ok(CompletedCommit::Appended(self.0.outcome.clone())),
            Path::ExactReplay => Ok(CompletedCommit::ExactReplay(self.0.outcome.clone())),
            Path::Conflict => Err(EventStoreError::new(
                EventStoreErrorKind::Conflict,
                "winner observed",
            )),
            Path::Lookup => panic!("lookup must not begin a session"),
        }
    }
}
#[async_trait]
impl ExecutionStore for FaultBackend {
    fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FaultSession(self)))
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
#[tokio::test]
async fn every_result_path_checks_full_reference_key_before_exposing_completion() {
    let authority = CommandAuthority::new("local").unwrap();
    let other_operation = CommandExecutionMetadata::new(
        OperationId::new("other").unwrap(),
        metadata().operation_fingerprint(),
    )
    .with_bounded_context(BoundedContextName::new("context").unwrap())
    .with_default_outcome_evidence::<()>();
    for outcome in [
        prepared(
            &CommandAuthority::new("foreign").unwrap(),
            &proposal(&metadata(), "foreign", vec![]),
        ),
        prepared(
            &authority,
            &proposal(
                &metadata().with_bounded_context(BoundedContextName::new("foreign").unwrap()),
                "foreign",
                vec![],
            ),
        ),
        prepared(
            &authority,
            &proposal(
                &other_operation
                    .with_correlation_id(CorrelationId::new("correlation").unwrap())
                    .with_causation_id(CausationId::new("cause").unwrap())
                    .with_security_scope(SecurityScope::new("scope").unwrap()),
                "foreign",
                vec![],
            ),
        ),
    ] {
        for path in [
            Path::Lookup,
            Path::Appended,
            Path::ExactReplay,
            Path::Conflict,
        ] {
            let backend = Arc::new(FaultBackend {
                authority: authority.clone(),
                outcome: outcome.clone(),
                path,
                committed: AtomicBool::new(false),
                begins: AtomicUsize::new(0),
            });
            let handler = Accept(AtomicUsize::new(0));
            let result = CommandExecutor::new(backend.clone())
                .with_max_conflict_retries(0)
                .execute_completed_with_rejection_codec(
                    &handler,
                    metadata(),
                    &(),
                    &JsonRejectionCodec::new("secret", 1),
                )
                .await;
            assert!(
                matches!(result, Err(CommandExecutionError::Store(ref e)) if e.kind() == EventStoreErrorKind::CorruptHistory),
                "{result:?}"
            );
            let attempts = usize::from(!matches!(path, Path::Lookup));
            assert_eq!(handler.0.load(Ordering::SeqCst), attempts);
            assert_eq!(backend.begins.load(Ordering::SeqCst), attempts);
        }
    }
}
