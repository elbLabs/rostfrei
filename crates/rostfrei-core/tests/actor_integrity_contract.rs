//! ACT-10..13: future `ActorRef` API, raw consistency and independent integrity boundaries.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
use async_trait::async_trait;
use rostfrei_core::*;
use rostfrei_messaging_core::{ActorRef, BoundedContextName};
use std::sync::{Arc, Mutex};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn actor(id: &str) -> ActorRef {
    ActorRef::new("people", id).unwrap()
}
fn metadata() -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new("actor-operation").unwrap(),
        ContentFingerprint::digest("same explicit content fingerprint"),
    )
    .with_bounded_context(BoundedContextName::new("actor-contract").unwrap())
}
struct Observe(Arc<Mutex<Vec<Option<ActorRef>>>>);
#[async_trait]
impl CommandHandler<()> for Observe {
    type Rejection = String;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0
            .lock()
            .unwrap()
            .push(execution.metadata().actor().cloned());
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::test]
async fn direct_typed_execution_compares_actor_even_with_identical_caller_supplied_fingerprint()
-> TestResult {
    let store = InMemoryEventStore::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Observe(seen.clone());
    let executor = CommandExecutor::new(store);
    let original = metadata().with_actor(actor("alice"));
    assert_eq!(
        original.operation_fingerprint(),
        metadata().operation_fingerprint()
    );
    let codec = JsonRejectionCodec::new("reason", 1);
    let first = executor
        .execute_completed_with_rejection_codec(&handler, original.clone(), &(), &codec)
        .await?;
    let second = executor
        .execute_completed_with_rejection_codec(&handler, original, &(), &codec)
        .await?;
    assert!(second.is_exact_replay());
    assert_eq!(first.outcome(), second.outcome());
    assert_eq!(first.outcome().marker().actor(), Some(&actor("alice")));
    for changed in [metadata(), metadata().with_actor(actor("bob"))] {
        let error = executor
            .execute_completed_with_rejection_codec(&handler, changed, &(), &codec)
            .await
            .unwrap_err();
        assert!(
            matches!(error, CommandExecutionError::Store(e) if e.kind() == EventStoreErrorKind::IdentityConflict)
        );
    }
    assert_eq!(*seen.lock().unwrap(), [Some(actor("alice"))]);
    Ok(())
}

fn batch() -> EventBatch {
    let id = stream();
    let m = metadata();
    let commit = derive_commit_id(&id, m.operation_id());
    EventBatch::new(
        commit.clone(),
        m.operation_id().clone(),
        m.operation_fingerprint(),
        vec![NewEvent::new(derive_event_id(&commit, 0), "added", 1, b"7".to_vec()).unwrap()],
    )
    .unwrap()
}
fn stream() -> StreamId {
    StreamId::new(
        AggregateType::new("counter").unwrap(),
        AggregateId::new("one").unwrap(),
    )
}
fn raw(batch_actor: Option<ActorRef>, transaction_actor: Option<ActorRef>) -> EventTransaction {
    let m = metadata();
    let mut b = batch();
    if let Some(actor) = batch_actor {
        b = b.with_actor(actor);
    }
    let mut tx = EventTransaction::new(
        m.operation_id().clone(),
        m.operation_fingerprint(),
        vec![TransactionParticipant::new(
            stream(),
            ExpectedVersion::NoStream,
            Some(b),
        )],
    )
    .with_bounded_context(m.bounded_context().unwrap().clone());
    if let Some(actor) = transaction_actor {
        tx = tx.with_actor(actor);
    }
    tx
}
#[tokio::test]
async fn raw_batch_and_transaction_attribution_must_agree_before_any_write() -> TestResult {
    for (batch_actor, tx_actor) in [
        (Some(actor("alice")), None),
        (None, Some(actor("alice"))),
        (Some(actor("alice")), Some(actor("bob"))),
    ] {
        let store = InMemoryEventStore::new();
        let error = store
            .append_transaction(raw(batch_actor, tx_actor))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), EventStoreErrorKind::InvalidRequest);
        assert!(store.load(&stream()).await?.is_empty());
        assert!(matches!(
            store
                .lookup_outcome(&OutcomeKey::new(
                    store.authority().clone(),
                    metadata().bounded_context().unwrap().clone(),
                    metadata().operation_id().clone()
                ))
                .await?,
            OutcomeLookup::Missing
        ));
    }
    for attribution in [None, Some(actor("alice"))] {
        let store = InMemoryEventStore::new();
        let tx = raw(attribution.clone(), attribution.clone());
        let first = store.append_transaction(tx.clone()).await?;
        assert_eq!(first.receipt().actor(), attribution.as_ref());
        assert_eq!(first.receipt().events()[0].actor(), attribution.as_ref());
        assert!(store.append_transaction(tx).await?.is_exact_replay());
        assert_eq!(
            store
                .append_transaction(raw(Some(actor("bob")), Some(actor("bob"))))
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
        );
        let direct = InMemoryEventStore::new();
        let mut b = batch();
        if let Some(actor) = attribution {
            b = b.with_actor(actor);
        }
        let first = direct
            .append(&stream(), ExpectedVersion::NoStream, b.clone())
            .await?;
        assert_eq!(first.events()[0].actor(), b.actor());
        assert!(
            direct
                .append(&stream(), ExpectedVersion::NoStream, b.clone())
                .await?
                .is_exact_replay()
        );
        assert_eq!(
            direct
                .append(
                    &stream(),
                    ExpectedVersion::NoStream,
                    b.with_actor(actor("bob"))
                )
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
        );
        assert_eq!(direct.load(&stream()).await?, first.events());
    }
    Ok(())
}

fn prepared(attribution: Option<ActorRef>, rejected: bool) -> TestResult<(OutcomeMarker, Vec<u8>)> {
    let m = metadata().with_default_outcome_evidence::<()>();
    let mut proposal =
        EventTransaction::new(m.operation_id().clone(), m.operation_fingerprint(), vec![])
            .with_bounded_context(m.bounded_context().unwrap().clone())
            .with_completion(CommandCompletion::new(
                m.outcome_evidence().cloned(),
                if rejected {
                    Some(JsonRejectionCodec::new("reason", 1).encode(&"exact reason".to_owned())?)
                } else {
                    None
                },
            ));
    let mut receipt =
        TransactionReceipt::new(m.operation_id().clone(), m.operation_fingerprint(), vec![])
            .with_bounded_context(m.bounded_context().unwrap().clone());
    if let Some(actor) = attribution {
        proposal = proposal.with_actor(actor.clone());
        receipt = receipt.with_actor(actor);
    }
    Ok(OutcomeMarker::prepare(
        CommandAuthority::new("actor-owned-authority")?,
        &proposal,
        receipt,
    )?)
}
#[test]
fn actor_binds_proposal_reference_details_manifest_revision_and_receipt_integrity() -> TestResult {
    for reject in [false, true] {
        let (original, bytes) = prepared(Some(actor("alice")), reject)?;
        let (other, other_bytes) = prepared(Some(actor("bob")), reject)?;
        assert_eq!(
            original.detail_reference().key(),
            other.detail_reference().key()
        );
        assert_ne!(original.proposal_digest(), other.proposal_digest());
        assert_eq!(original.proposal_digest().version(), 2);
        assert_ne!(
            original.detail_reference().encode()?,
            other.detail_reference().encode()?
        );
        assert_ne!(original.revision(), other.revision());
        assert_eq!(original.detail_reference().actor(), Some(&actor("alice")));
        assert!(EncodedOutcomeDetails::decode(original.detail_reference(), &other_bytes).is_err());
        let details = EncodedOutcomeDetails::decode(original.detail_reference(), &bytes)?;
        let inline = CompletedOutcome::new(original.clone(), OutcomeDetails::Available(details))?
            .to_inline()?;
        assert_eq!(inline.record().actor(), original.actor());
        let manifest = original.encode_manifest()?;
        let wire: serde_json::Value = serde_json::from_slice(&manifest)?;
        assert_eq!(wire["version"], 2);
        assert_eq!(wire["format"], 2);
        assert_eq!(wire["actor"], serde_json::to_value(actor("alice"))?);
        assert_eq!(
            OutcomeMarker::restore_manifest(
                original.detail_reference().key(),
                &manifest,
                original.integrity_receipt().clone()
            )?,
            original
        );
        for mutation in ["changed", "absent", "null", "downgrade"] {
            let mut changed = wire.clone();
            match mutation {
                "changed" => changed["actor"] = serde_json::to_value(actor("bob"))?,
                "absent" => {
                    changed.as_object_mut().unwrap().remove("actor");
                }
                "null" => changed["actor"] = serde_json::Value::Null,
                _ => changed["version"] = serde_json::json!(1),
            }
            assert!(
                OutcomeMarker::restore_manifest(
                    original.detail_reference().key(),
                    &serde_json::to_vec(&changed)?,
                    original.integrity_receipt().clone()
                )
                .is_err(),
                "{mutation}"
            );
        }
        assert!(
            OutcomeMarker::restore_manifest(
                original.detail_reference().key(),
                &manifest,
                original
                    .integrity_receipt()
                    .clone()
                    .with_actor(actor("bob"))
            )
            .is_err()
        );
    }
    // Independently assembled event/receipt: internally consistent coordinates, contradictory actor.
    let b = batch().with_actor(actor("alice"));
    let event = RecordedEvent::new(
        stream(),
        StreamVersion::new(1),
        b.events()[0].event_id().clone(),
        b.commit_id().clone(),
        b.operation_id().clone(),
        b.operation_fingerprint(),
        "added",
        1,
        b"7".to_vec(),
    )?
    .with_actor(actor("bob"));
    let receipt = TransactionReceipt::new(
        b.operation_id().clone(),
        b.operation_fingerprint(),
        vec![TransactionStreamReceipt::new(
            stream(),
            StreamVersion::ZERO,
            vec![event],
        )],
    )
    .with_actor(actor("alice"));
    assert_eq!(
        receipt.validate_integrity().unwrap_err().kind(),
        EventStoreErrorKind::CorruptHistory
    );
    Ok(())
}

#[test]
fn actorless_legacy_bytes_keep_their_original_hash_version_and_unknown_attribution() -> TestResult {
    let vector = EventTransaction::new(
        OperationId::new("vector")?,
        ContentFingerprint::digest("command"),
        vec![],
    );
    let digest = ProposalDigest::of(&vector)?;
    assert_eq!(digest.version(), 1);
    assert_eq!(
        digest.fingerprint().to_hex(),
        "3bd8f1acdbbd5888ad7da3e615ae446663df585670594e0266f78241e4b2355b"
    );
    let (legacy, bytes) = prepared(None, true)?;
    let manifest = legacy.encode_manifest()?;
    let wire: serde_json::Value = serde_json::from_slice(&manifest)?;
    assert_eq!(wire["version"], 1);
    assert_eq!(wire["format"], 1);
    assert!(wire.get("actor").is_none());
    let restored = OutcomeMarker::restore_manifest(
        legacy.detail_reference().key(),
        &manifest,
        legacy.integrity_receipt().clone(),
    )?;
    assert_eq!(restored.actor(), None);
    assert_eq!(restored.encode_manifest()?, manifest);
    EncodedOutcomeDetails::decode(restored.detail_reference(), &bytes)?;
    let mut injection = wire;
    injection["actor"] = serde_json::to_value(actor("alice"))?;
    assert!(
        OutcomeMarker::restore_manifest(
            legacy.detail_reference().key(),
            &serde_json::to_vec(&injection)?,
            legacy.integrity_receipt().clone()
        )
        .is_err()
    );
    Ok(())
}
