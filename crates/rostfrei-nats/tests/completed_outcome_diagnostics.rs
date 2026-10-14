//! A technical projection is not an authority over a subsequently durable completion.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
rostfrei::install_macro_support!();

fn response_stream_name(fixture: &Fixture) -> &str {
    fixture
        .messaging
        .topology()
        .command_response_stream()
        .as_str()
}

async fn completed_winner_after_blocked_diagnostic(accepted: bool) -> TestResult {
    let fixture = Fixture::new("92-completion-after-blocked-diagnostic").await?;
    let store = Arc::new(FaultStore {
        inner: InMemoryEventStore::new(),
        kind: EventStoreErrorKind::CorruptHistory,
        active: Arc::new(AtomicBool::new(true)),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let encoded = local_bus(&fixture.context, processor.clone()).encode(request(
        "diagnostic-operation",
        "bad-target",
        if accepted { "create" } else { "add" },
    )?)?;
    let operation = OperationId::new(encoded.operation_id().as_str())?;
    let handler = fixture.adapter.command_handler(processor.clone());
    assert!(matches!(
        handler.handle(command_delivery(&encoded)?).await,
        DeliveryDisposition::Quarantine(_)
    ));
    let responses = fixture
        .js
        .get_stream(response_stream_name(&fixture))
        .await?;
    let diagnostic = responses
        .get_last_raw_message_by_subject(encoded.response_address()?.as_str())
        .await?;
    let body: CommandResponse = serde_json::from_slice(&diagnostic.payload)?;
    assert_eq!(
        body.outcome(),
        &CommandResponseOutcome::ExecutionFailed(ExecutionFailure::Store(
            StoreFailureKind::CorruptHistory
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    store.active.store(false, Ordering::SeqCst);
    // Repair alone still cannot silently re-evaluate a blocked technical delivery.
    assert!(matches!(
        handler.handle(command_delivery(&encoded)?).await,
        DeliveryDisposition::Quarantine(_)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        store
            .inner
            .load_completed_outcome_in_context(fixture.context.name(), &operation)
            .await?
            .is_none()
    );

    // A direct entry point can complete the unchanged operation against repaired state.
    let response = processor.process(&encoded).await?;
    assert_eq!(
        matches!(response.outcome(), CommandResponseOutcome::Accepted),
        accepted
    );
    assert_eq!(rejected(response.outcome()), !accepted);
    let authority = store
        .inner
        .load_completed_outcome_in_context(fixture.context.name(), &operation)
        .await?
        .unwrap();
    assert!(
        matches!(
            handler.handle(command_delivery(&encoded)?).await,
            DeliveryDisposition::Acknowledge
        ),
        "a cached technical diagnostic must not override shared completed authority"
    );
    let reader =
        NatsCommandResponseReader::new(fixture.js.clone(), fixture.messaging.topology().clone());
    assert_eq!(
        reader
            .read_command_response(
                &encoded.response_address()?,
                encoded.operation_id(),
                encoded.message_id(),
                WAIT
            )
            .await?,
        response
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "reconciliation must not re-run the handler"
    );
    assert_eq!(
        store
            .inner
            .load_completed_outcome_in_context(fixture.context.name(), &operation)
            .await?
            .unwrap(),
        authority
    );
    assert_eq!(
        store.inner.load(&stream("bad-target")?).await?.len(),
        usize::from(accepted)
    );
    assert_eq!(
        responses
            .get_raw_message(diagnostic.sequence)
            .await?
            .payload,
        diagnostic.payload,
        "the historical diagnostic remains retained for audit, not deleted by reconciliation"
    );
    Ok(())
}

#[tokio::test]
async fn durable_acceptance_supersedes_blocked_technical_projection_without_reevaluation()
-> TestResult {
    completed_winner_after_blocked_diagnostic(true).await
}

#[tokio::test]
async fn durable_rejection_supersedes_blocked_technical_projection_without_reevaluation()
-> TestResult {
    completed_winner_after_blocked_diagnostic(false).await
}
