//! Legacy-only response evidence is imported or blocked, never silently reevaluated.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
rostfrei_runtime::install_macro_support!();

async fn legacy(
    label: &str,
) -> TestResult<(
    Fixture,
    Arc<NatsEventStore>,
    rostfrei_runtime::EncodedCommand,
    CommandResponse,
)> {
    let fixture = Fixture::new(label).await?;
    let config = NatsEventStoreConfig::for_bounded_context(&fixture.context)?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    provision_event_store(&fixture.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(fixture.js.clone(), config).await?);
    let command = CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
        .encode(request("legacy-rejected", "target", "add")?)?;
    // Exact baseline schema-1 business response, with no shared acceptance/rejection.
    let response = CommandResponse::rejected(
        rostfrei_runtime::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        map_rejection(&"target-not-found")?,
    )?;
    fixture
        .publisher()
        .publish_command_response(
            OutboundMessage::new(
                command.response_address()?,
                response.message_id().clone(),
                serde_json::to_vec(&response)?,
            )?
            .with_correlation_id(command.correlation_id().clone()),
        )
        .await?;
    assert!(
        store
            .load_transaction_receipt_in_context(
                fixture.context.name(),
                &OperationId::new("legacy-rejected")?
            )
            .await?
            .is_none()
    );
    // Repair via the legacy low-level event API using a DIFFERENT operation, before cutover.
    let id = stream("target")?;
    let operation = OperationId::new("legacy-repair")?;
    let commit = derive_commit_id(&id, &operation);
    let event = NewEvent::new(
        derive_event_id(&commit, 0),
        "changed",
        1,
        serde_json::to_vec(&Changed {
            created: true,
            item: None,
        })?,
    )?;
    store
        .append(
            &id,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest("repair"),
                vec![event],
            )?,
        )
        .await?;
    Ok((fixture, store, command, response))
}

#[tokio::test]
async fn original_evidence_import_replays_legacy_rejection_without_repaired_state_mutation()
-> TestResult {
    let (fixture, store, command, response) = legacy("92-legacy-import").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    assert!(
        processor.process(&command).await.is_err(),
        "legacy namespace must block before handler"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    store
        .import_legacy_rejected_outcome(&command, &response, fixture.messaging.topology())
        .await?;
    assert_eq!(processor.process(&command).await?, response);
    let recreated = processor_for(&fixture, store.clone(), calls.clone())?;
    assert_eq!(recreated.process(&command).await?, response);
    let disposition = fixture
        .adapter
        .command_handler(recreated)
        .handle(command_delivery(&command)?)
        .await;
    assert_eq!(disposition, DeliveryDisposition::Acknowledge);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    assert!(
        store
            .import_legacy_rejected_outcome(&command, &response, fixture.messaging.topology())
            .await?
            .is_exact_replay()
    );
    Ok(())
}

fn processor_for(
    fixture: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<CommandProcessor>> {
    processor(&fixture.context, store, calls, None)
}

#[tokio::test]
async fn missing_conflicting_or_invisible_legacy_evidence_stays_blocked() -> TestResult {
    let (fixture, store, command, response) = legacy("92-legacy-block").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let bus = local_bus(&fixture.context, processor.clone());
    let changed = bus.encode(request("legacy-rejected", "other-target", "add")?)?;
    assert!(
        store
            .import_legacy_rejected_outcome(&changed, &response, fixture.messaging.topology())
            .await
            .is_err()
    );
    assert!(processor.process(&command).await.is_err());
    let invisible = bus.encode(request("expired-original-evidence", "target", "add")?)?;
    assert!(processor.process(&invisible).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let fresh = store
        .config()
        .clone()
        .with_fresh_completed_outcome_epoch("cannot-infer-freshness")?;
    assert_eq!(
        provision_event_store(&fixture.js, &fresh)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(
        update_event_store(&fixture.js, &fresh)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    store
        .import_legacy_rejected_outcome(&command, &response, fixture.messaging.topology())
        .await?;
    let contradictory = CommandResponse::rejected(
        response.message_id().clone(),
        response.command_message_id().clone(),
        response.command_address().clone(),
        response.operation_id().clone(),
        response.correlation_id().clone(),
        CommandRejection::new(
            CommandRejectionClassification::Conflict,
            ApplicationErrorCode::new("legacy.other")?,
            "contradictory retained rejection",
            None,
        )?,
    )?;
    assert_eq!(
        store
            .import_legacy_rejected_outcome(&command, &contradictory, fixture.messaging.topology())
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );
    assert_eq!(processor.process(&command).await?, response);
    assert!(
        processor.process(&invisible).await.is_err(),
        "importing one identity must not activate invisible history"
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn legacy_acceptance_cannot_be_overwritten_by_imported_rejection() -> TestResult {
    let (fixture, store, command, response) = legacy("92-legacy-accept-conflict").await?;
    let original = command.legacy_rejection_transaction(&response)?;
    let accepted = EventTransaction::new(
        original.operation_id().clone(),
        original.operation_fingerprint(),
        Vec::new(),
    )
    .with_bounded_context(fixture.context.name().clone())
    .with_correlation_id(original.correlation_id().unwrap().clone())
    .with_causation_id(original.causation_id().unwrap().clone());
    store.append_transaction(accepted).await?;
    assert_eq!(
        store
            .import_legacy_rejected_outcome(&command, &response, fixture.messaging.topology())
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );
    assert!(
        store
            .load_transaction_receipt_in_context(fixture.context.name(), original.operation_id())
            .await?
            .is_some()
    );
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(
        processor(&fixture.context, store, calls.clone(), None)?
            .process(&command)
            .await
            .is_err(),
        "contradictory legacy acceptance/rejection must block, not select acceptance"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}
