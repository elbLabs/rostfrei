#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod release_contract_support;
use release_contract_support::*;
rostfrei_runtime::install_macro_support!();

async fn append(
    store: &NatsEventStore,
    tx: EventTransaction,
) -> Result<TransactionAppendOutcome, EventStoreError> {
    if tx.completion().is_none() {
        return store.append_transaction(tx).await;
    }
    let completed = store.commit_outcome(tx).await?;
    let receipt = completed.outcome().record().clone();
    Ok(if completed.is_exact_replay() {
        TransactionAppendOutcome::ExactReplay(receipt)
    } else {
        TransactionAppendOutcome::Appended(receipt)
    })
}

fn transaction(id: &str, write: bool, actor: Option<ActorRef>) -> TestResult<EventTransaction> {
    let operation = OperationId::new(id)?;
    let fingerprint = ContentFingerprint::digest("explicit fingerprint");
    let stream = stream(id)?;
    let batch = if write {
        let commit = derive_commit_id(&stream, &operation);
        let mut batch = EventBatch::new(
            commit.clone(),
            operation.clone(),
            fingerprint,
            vec![NewEvent::new(
                derive_event_id(&commit, 0),
                "changed",
                1,
                br#"{"created":true,"item":null}"#.to_vec(),
            )?],
        )?;
        if let Some(actor) = &actor {
            batch = batch.with_actor(actor.clone());
        }
        Some(batch)
    } else {
        None
    };
    let mut tx = EventTransaction::new(
        operation,
        fingerprint,
        vec![TransactionParticipant::new(
            stream,
            ExpectedVersion::NoStream,
            batch,
        )],
    );
    if let Some(actor) = actor {
        tx = tx.with_actor(actor);
    }
    Ok(tx)
}

#[tokio::test]
async fn attributed_raw_and_inline_receipts_coexist_with_unknown_actors() -> TestResult {
    let f = Fixture::new("actor-receipt-paths").await?;
    let store = f.event_store().await?;
    for actor in [None, Some(ActorRef::new("people", "alice")?)] {
        for shape in [
            "raw-write",
            "raw-guard",
            "inline-write",
            "inline-noevent",
            "inline-reject",
        ] {
            let id = format!("{shape}-{}", actor.is_some());
            let mut tx = transaction(&id, shape.ends_with("write"), actor.clone())?
                .with_bounded_context(f.context.name().clone());
            if shape.starts_with("inline") {
                let evidence = CommandExecutionMetadata::new(
                    tx.operation_id().clone(),
                    tx.operation_fingerprint(),
                )
                .with_default_outcome_evidence::<()>();
                tx = tx.with_completion(CommandCompletion::new(
                    evidence.outcome_evidence().cloned(),
                    if shape == "inline-reject" {
                        Some(JsonRejectionCodec::new("reason", 1).encode(&"no".to_owned())?)
                    } else {
                        None
                    },
                ));
            }
            let first = append(&store, tx.clone()).await?;
            assert_eq!(first.receipt().actor(), actor.as_ref());
            first.receipt().validate_integrity()?;
            assert_eq!(first.receipt().completion(), tx.completion());
            let reconnected = NatsEventStore::connect(f.js.clone(), store.config().clone()).await?;
            let loaded = reconnected
                .load_completed_outcome_in_context(f.context.name(), tx.operation_id())
                .await?
                .ok_or("missing receipt")?;
            assert_eq!(loaded.record(), first.receipt());
            assert!(append(&reconnected, tx.clone()).await?.is_exact_replay());
            let journal = f.js.get_stream(store.config().stream_name()).await?;
            let raw = journal
                .get_raw_message(journal.cached_info().state.last_sequence)
                .await?;
            let wire: serde_json::Value = serde_json::from_slice(&raw.payload)?;
            assert_eq!(
                wire["schemaVersion"],
                if actor.is_some() {
                    5
                } else if shape.starts_with("inline") {
                    3
                } else if shape.ends_with("write") {
                    1
                } else {
                    2
                }
            );
            let changed = transaction(
                &id,
                shape.ends_with("write"),
                Some(ActorRef::new("people", "bob")?),
            )?
            .with_bounded_context(f.context.name().clone());
            let changed = if let Some(completion) = tx.completion() {
                changed.with_completion(completion.clone())
            } else {
                changed
            };
            assert_eq!(
                append(&reconnected, changed).await.unwrap_err().kind(),
                EventStoreErrorKind::IdentityConflict
            );
            let history = reconnected.load(&stream(&id)?).await?;
            assert_eq!(history, first.receipt().events());
            assert_eq!(reconnected.audit_history(&stream(&id)?).await?, history);
            for event in history {
                assert_eq!(event.actor(), actor.as_ref());
            }
        }
    }
    // Conflicting raw attribution must refuse before preparing any event or receipt.
    let before = f.messages(store.config().stream_name()).await?;
    for id in ["contradictory", "raw-write-true"] {
        let inconsistent =
            transaction(id, true, None)?.with_actor(ActorRef::new("people", "alice")?);
        assert_eq!(
            store
                .append_transaction(inconsistent)
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::InvalidRequest
        );
    }
    assert_eq!(f.messages(store.config().stream_name()).await?, before);
    assert!(store.load(&stream("contradictory")?).await?.is_empty());
    Ok(())
}
