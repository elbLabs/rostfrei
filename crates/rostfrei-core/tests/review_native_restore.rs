//! Independently challenge revision-v2 and restoration with nontrivial event integrity.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#![allow(
    clippy::indexing_slicing,
    reason = "fixed two-participant independent fixture"
)]
use rostfrei_core::*;
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};

fn proposal(base: u64, payload: &[u8]) -> EventTransaction {
    let operation = OperationId::new("same-operation").unwrap();
    let fp = ContentFingerprint::digest("same-command");
    let context = BoundedContextName::new("review").unwrap();
    let scope = SecurityScope::new("scope").unwrap();
    let correlation = CorrelationId::new("correlation").unwrap();
    let causation = CausationId::new("cause").unwrap();
    let id = StreamId::new(
        AggregateType::new("counter").unwrap(),
        AggregateId::new("writer").unwrap(),
    );
    let commit = derive_commit_id(&id, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation.clone(),
        fp,
        vec![
            NewEvent::new(derive_event_id(&commit, 0), "changed", 1, payload.to_vec()).unwrap(),
            NewEvent::new(
                derive_event_id(&commit, 1),
                "changed",
                2,
                b"second".to_vec(),
            )
            .unwrap(),
        ],
    )
    .unwrap()
    .with_correlation_id(correlation.clone())
    .with_causation_id(causation.clone());
    let evidence = CommandExecutionMetadata::new(operation.clone(), fp)
        .with_default_outcome_evidence::<()>()
        .outcome_evidence()
        .cloned();
    EventTransaction::new(
        operation,
        fp,
        vec![
            TransactionParticipant::new(id, ExpectedVersion::NoStream, Some(batch)),
            TransactionParticipant::new(
                StreamId::new(
                    AggregateType::new("counter").unwrap(),
                    AggregateId::new("guard").unwrap(),
                ),
                if base == 0 {
                    ExpectedVersion::NoStream
                } else {
                    ExpectedVersion::Exact(StreamVersion::new(base))
                },
                None,
            ),
        ],
    )
    .with_bounded_context(context)
    .with_security_scope(scope)
    .with_correlation_id(correlation)
    .with_causation_id(causation)
    .with_completion(CommandCompletion::new(evidence, None))
}

#[tokio::test]
async fn restoration_binds_events_and_guard_bases_even_when_proposal_digest_excludes_versions() {
    let store = InMemoryEventStore::new();
    let p = proposal(0, b"first");
    let committed = store
        .begin_command()
        .await
        .unwrap()
        .commit_outcome(p.clone())
        .await
        .unwrap();
    let original = committed.outcome().marker();
    let receipt = original.integrity_receipt();
    let bytes = original.encode_manifest().unwrap();
    assert_eq!(
        OutcomeMarker::restore_manifest(original.detail_reference().key(), &bytes, receipt.clone())
            .unwrap(),
        *original
    );
    // Same immutable command details; a different guard base must still change the revision.
    let changed_guard = proposal(7, b"first");
    assert_eq!(
        ProposalDigest::of(&p).unwrap(),
        ProposalDigest::of(&changed_guard).unwrap()
    );
    let changed_receipt = TransactionReceipt::new(
        receipt.operation_id().clone(),
        receipt.operation_fingerprint(),
        vec![
            receipt.streams()[0].clone(),
            TransactionStreamReceipt::new(
                receipt.streams()[1].stream_id().clone(),
                StreamVersion::new(7),
                vec![],
            ),
        ],
    )
    .with_bounded_context(receipt.bounded_context().unwrap().clone())
    .with_security_scope(receipt.security_scope().unwrap().clone())
    .with_correlation_id(receipt.correlation_id().unwrap().clone())
    .with_causation_id(receipt.causation_id().unwrap().clone());
    let (other, _) = OutcomeMarker::prepare(
        store.authority().clone(),
        &changed_guard,
        changed_receipt.clone(),
    )
    .unwrap();
    assert_eq!(original.detail_reference(), other.detail_reference());
    assert_ne!(original.revision(), other.revision());
    assert!(
        OutcomeMarker::restore_manifest(original.detail_reference().key(), &bytes, changed_receipt)
            .is_err()
    );
    assert!(
        OutcomeMarker::restore_manifest(
            original.detail_reference().key(),
            &other.encode_manifest().unwrap(),
            receipt.clone()
        )
        .is_err()
    );
    // Independent valid transaction with the same immutable detail reference but changed events.
    let other_store = InMemoryEventStore::new();
    let changed = other_store
        .begin_command()
        .await
        .unwrap()
        .commit_outcome(proposal(0, b"other"))
        .await
        .unwrap();
    let (changed_event, _) = OutcomeMarker::prepare(
        store.authority().clone(),
        &proposal(0, b"other"),
        changed.outcome().marker().integrity_receipt().clone(),
    )
    .unwrap();
    assert_eq!(
        original.detail_reference(),
        changed_event.detail_reference()
    );
    assert_ne!(original.proposal_digest(), changed_event.proposal_digest());
    assert_ne!(original.revision(), changed_event.revision());
    assert!(
        OutcomeMarker::restore_manifest(
            original.detail_reference().key(),
            &bytes,
            changed_event.integrity_receipt().clone()
        )
        .is_err()
    );
    let expiry = store
        .expire_details(original.detail_reference().key(), other.revision())
        .await;
    assert_eq!(expiry.unwrap_err(), OutcomeLifecycleError::RevisionMismatch);
    assert!(
        matches!(store.lookup_outcome(original.detail_reference().key()).await.unwrap(), OutcomeLookup::Completed(v) if matches!(v.details(), OutcomeDetails::Available(_)))
    );
}
