#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use rostfrei::EncodedCommand;
rostfrei::install_macro_support!();

fn rebuild(
    original: &EncodedCommand,
    address: CommandAddress,
    value: serde_json::Value,
) -> TestResult<EncodedCommand> {
    let provisional = EncodedCommand::from_delivery(
        address.clone(),
        original.message_id().clone(),
        serde_json::to_vec(&value)?,
    )?;
    let correlation = CorrelationId::new(
        value
            .get("correlation_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("correlation absent")?,
    )?;
    let causation = value
        .get("causation_id")
        .and_then(serde_json::Value::as_str)
        .map(CausationId::new)
        .transpose()?;
    let operation = rostfrei_messaging_core::OperationId::new(
        value
            .get("operation_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("operation absent")?,
    )?;
    let id = rostfrei::command_message_id(
        &address,
        &operation,
        provisional.fingerprint(),
        &correlation,
        causation.as_ref(),
    )?;
    let mut value = value;
    set(&mut value, "/message_id", id.as_str().into())?;
    let command = EncodedCommand::from_delivery(address, id, serde_json::to_vec(&value)?)?;
    command.validate()?;
    Ok(command)
}

fn set(value: &mut serde_json::Value, pointer: &str, replacement: serde_json::Value) -> TestResult {
    *value
        .pointer_mut(pointer)
        .ok_or("fixture JSON pointer absent")? = replacement;
    Ok(())
}

fn changed(original: &EncodedCommand, dimension: &str) -> TestResult<EncodedCommand> {
    let mut value: serde_json::Value = serde_json::from_slice(original.payload())?;
    let mut address = original.address().clone();
    match dimension {
        "payload" => set(&mut value, "/payload/payload/item", "DIFFERENT".into())?,
        "command" => {
            set(&mut value, "/payload/command", "unregistered".into())?;
            address = CommandAddress::new_in_scope(
                original.address().application(),
                original.address().traffic_scope(),
                original.address().context(),
                "unregistered",
            )?;
        }
        "schema" => {
            set(&mut value, "/schema_version", 2.into())?;
            set(&mut value, "/payload/schema_version", 2.into())?;
        }
        "provenance" => {
            value
                .get_mut("payload")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or("missing routed command")?
                .insert("events_caused_by_command".to_owned(), true.into());
        }
        "correlation" => set(
            &mut value,
            "/correlation_id",
            "different-correlation".into(),
        )?,
        "causation" => set(&mut value, "/causation_id", "different-causation".into())?,
        "application" => {
            address =
                CommandAddress::new("foreign-application", original.address().context(), "act")?;
        }
        "traffic" => {
            address = CommandAddress::new_in_scope(
                original.address().application(),
                TrafficScope::Test,
                original.address().context(),
                "act",
            )?;
        }
        _ => return Err("unknown fixture dimension".into()),
    }
    rebuild(original, address, value)
}

async fn matrix(accepted: bool) -> TestResult {
    let fixture = Fixture::new(if accepted {
        "92-evidence-accepted"
    } else {
        "92-evidence-rejected"
    })
    .await?;
    let store = fixture.event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let bus = local_bus(&fixture.context, processor.clone());
    let original = bus.encode(request(
        "evidence-operation",
        "missing",
        if accepted { "noop" } else { "add" },
    )?)?;
    let response = processor.process(&original).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let value: serde_json::Value = serde_json::from_slice(original.payload())?;
    for dimension in [
        "payload",
        "command",
        "schema",
        "provenance",
        "correlation",
        "causation",
        "application",
        "traffic",
    ] {
        let command = changed(&original, dimension)?;
        let result = processor.process(&command).await?;
        assert!(
            matches!(result.outcome(), CommandResponseOutcome::Rejected(rejection) if rejection.classification() == CommandRejectionClassification::Conflict),
            "{dimension}: {result:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "{dimension} must fail before handler"
        );
    }
    let mut timestamp = value.clone();
    set(&mut timestamp, "/created_at", 1_700_000_000_999_u64.into())?;
    assert_eq!(
        processor
            .process(&rebuild(&original, original.address().clone(), timestamp)?)
            .await?,
        response
    );
    let reformatted = EncodedCommand::from_delivery(
        original.address().clone(),
        original.message_id().clone(),
        serde_json::to_vec_pretty(&value)?,
    )?;
    assert_eq!(
        processor.process(&reformatted).await?,
        response,
        "JSON presentation is not operation identity"
    );
    let recorded = store
        .load_completed_outcome_in_context(
            fixture.context.name(),
            &OperationId::new("evidence-operation")?,
        )
        .await?
        .ok_or("missing outcome")?;
    assert_eq!(
        recorded
            .record()
            .completion()
            .unwrap()
            .evidence
            .as_ref()
            .unwrap()
            .created_at_millis,
        Some(1_700_000_000_000)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(store.load(&stream("missing")?).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn accepted_complete_identity_matrix_and_audit_timestamp() -> TestResult {
    matrix(true).await
}
#[tokio::test]
async fn rejected_complete_identity_matrix_and_audit_timestamp() -> TestResult {
    matrix(false).await
}

async fn expired_projection(accepted: bool) -> TestResult {
    let fixture = Fixture::new("92-transport-expiry").await?;
    let store = fixture.event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let first = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let worker = fixture.command_worker(first).await?;
    let mut responses = fixture
        .js
        .get_stream(
            fixture
                .messaging
                .topology()
                .command_response_stream()
                .as_str(),
        )
        .await?;
    let mut response_config = responses.info().await?.config.clone();
    response_config.max_age = Duration::from_millis(150);
    response_config.duplicate_window = Duration::from_millis(100);
    fixture.js.update_stream(response_config).await?;
    let mut commands = fixture
        .js
        .get_stream(fixture.messaging.topology().command_stream().as_str())
        .await?;
    let mut command_config = commands.info().await?.config.clone();
    command_config.duplicate_window = Duration::from_millis(100);
    fixture.js.update_stream(command_config).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let request = request(
        "ttl-rejection",
        "target",
        if accepted { "create" } else { "add" },
    )?;
    let encoded = remote.encode(request.clone())?;
    let original = remote.dispatch(request).await?;
    assert_eq!(rejected(original.response().outcome()), !accepted);
    tokio::time::timeout(WAIT, async {
        loop {
            if responses.info().await?.state.messages == 0 {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    drop(worker);
    let recreated = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, recreated.clone());
    if !accepted {
        local
            .dispatch(release_contract_support::request(
                "ttl-repair",
                "target",
                "create",
            )?)
            .await?;
    }
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(recreated.process(&encoded).await?, *original.response());
    let _worker = fixture.command_worker(recreated).await?;
    let replay = remote.dispatch_encoded(encoded).await?;
    assert!(
        !replay.publication_duplicate(),
        "broker duplicate window must actually have expired"
    );
    assert_eq!(replay.response(), original.response());
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn delayed_rejected_retry_after_actual_response_ttl_and_publication_dedup_expiry()
-> TestResult {
    expired_projection(false).await
}
#[tokio::test]
async fn delayed_accepted_retry_after_actual_response_ttl_and_publication_dedup_expiry()
-> TestResult {
    expired_projection(true).await
}
