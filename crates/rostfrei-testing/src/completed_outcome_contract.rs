//! Low-level atomic completed-decision conformance shared by memory and real NATS.
#![allow(clippy::panic_in_result_fn)]
use rostfrei_core::{
    AggregateId, AggregateType, CommandCompletion, CompletedAppendOutcome, ContentFingerprint,
    EncodedRejection, EventStore, EventStoreErrorKind, EventTransaction, ExpectedVersion,
    OperationId, StreamId, TransactionParticipant,
};
use rostfrei_messaging_core::BoundedContextName;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn transaction(
    context: &BoundedContextName,
    operation: &str,
    rejected: bool,
) -> Result<EventTransaction, Box<dyn std::error::Error + Send + Sync>> {
    Ok(EventTransaction::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
        Vec::new(),
    )
    .with_bounded_context(context.clone())
    .with_completion(CommandCompletion::new(
        None,
        rejected.then(|| EncodedRejection {
            codec_id: "conformance.binary".to_owned(),
            codec_version: 1,
            bytes: vec![0, 255, 1],
        }),
    )))
}

pub async fn run(store: &dyn EventStore, context: &BoundedContextName) -> TestResult {
    let missing = StreamId::new(AggregateType::new("guarded")?, AggregateId::new("missing")?);
    let mut rejected = transaction(context, "conformance-rejected", true)?;
    let (operation, fingerprint) = (
        rejected.operation_id().clone(),
        rejected.operation_fingerprint(),
    );
    rejected = EventTransaction::new(
        operation.clone(),
        fingerprint,
        vec![TransactionParticipant::new(
            missing.clone(),
            ExpectedVersion::NoStream,
            None,
        )],
    )
    .with_bounded_context(context.clone())
    .with_completion(rejected.completion().ok_or("missing completion")?.clone());
    let session = store.append_session().await?;
    assert!(session.load(&missing).await?.is_empty());
    assert!(matches!(
        session.commit_outcome(rejected.clone()).await?,
        CompletedAppendOutcome::Appended(_)
    ));
    let stored = store
        .load_completed_outcome_in_context(context, &operation)
        .await?
        .ok_or("missing rejected winner")?;
    assert!(stored.acceptance().is_none());
    assert_eq!(
        stored.rejection().ok_or("missing bytes")?.bytes,
        vec![0, 255, 1]
    );
    assert!(store.load(&missing).await?.is_empty());
    assert_eq!(stored.record().streams().len(), 1);
    assert!(matches!(
        store.commit_outcome(rejected.clone()).await?,
        CompletedAppendOutcome::ExactReplay(_)
    ));
    assert_eq!(
        store
            .load_transaction_receipt_in_context(context, &operation)
            .await
            .expect_err("rejection must not decode as acceptance")
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );
    let changed =
        EventTransaction::new(operation, ContentFingerprint::digest("changed"), Vec::new())
            .with_bounded_context(context.clone())
            .with_completion(CommandCompletion::new(None, None));
    assert_eq!(
        store
            .commit_outcome(changed)
            .await
            .expect_err("cannot replace rejection")
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );

    let accepted = transaction(context, "conformance-arbitration", false)?;
    let rejected = transaction(context, "conformance-arbitration", true)?;
    let (a, b) = tokio::join!(
        store.commit_outcome(accepted.clone()),
        store.commit_outcome(rejected.clone())
    );
    assert_ne!(
        a.is_ok(),
        b.is_ok(),
        "exactly one accepted/rejected atomic winner: {a:?} {b:?}"
    );
    let winner = store
        .load_completed_outcome_in_context(context, accepted.operation_id())
        .await?
        .ok_or("missing contender winner")?;
    assert_eq!(winner.rejection().is_none(), a.is_ok());
    let loser = if a.is_ok() { b } else { a };
    assert_eq!(
        loser.expect_err("loser must conflict").kind(),
        EventStoreErrorKind::IdentityConflict
    );
    let mut bad = transaction(context, "conformance-version", true)?;
    let mut completion = bad.completion().ok_or("missing version")?.clone();
    completion.version = 999;
    bad = bad.with_completion(completion);
    assert_eq!(
        store
            .commit_outcome(bad)
            .await
            .expect_err("unknown schema must fail")
            .kind(),
        EventStoreErrorKind::InvalidRequest
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn memory_completed_outcomes() -> super::TestResult {
        super::run(
            &rostfrei_core::InMemoryEventStore::new(),
            &rostfrei_messaging_core::BoundedContextName::new("conformance")?,
        )
        .await
    }
}
