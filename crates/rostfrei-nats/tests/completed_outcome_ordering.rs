//! A concurrent winner must not bypass an already-started session's invariant checks.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
rostfrei::install_macro_support!();

struct GatedRejection {
    calls: Arc<AtomicUsize>,
    reached: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Semaphore>,
}

#[async_trait]
impl CommandHandler<Act> for GatedRejection {
    type Rejection = &'static str;

    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let decision = Handler(self.calls.clone())
            .handle(command, execution)
            .await?;
        self.reached.notify_one();
        self.release.acquire().await.unwrap().forget();
        Ok(decision)
    }
}

fn decode_rejection(rejection: &CommandRejection) -> Result<&'static str, String> {
    if rejection.code().as_str() == "contract.target-not-found" {
        Ok("target-not-found")
    } else {
        Err("unknown stored rejection".to_owned())
    }
}

async fn recreated_session_winner(accepted: bool) -> TestResult {
    let fixture = Fixture::new("92-rejected-session-winner-ordering").await?;
    let store = fixture.event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let original_processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let encoded = local_bus(&fixture.context, original_processor).encode(request(
        "session-operation",
        "target",
        "add",
    )?)?;
    let metadata = encoded.validated_execution_metadata()?;
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let handler = GatedRejection {
        calls: calls.clone(),
        reached: reached.clone(),
        release: release.clone(),
    };
    let executor = CommandExecutor::new(store.clone());
    let task = tokio::spawn(async move {
        let command = Act {
            target: "target".to_owned(),
            action: "add".to_owned(),
            item: "I".to_owned(),
        };
        let codec = rostfrei::MappedCommandRejectionCodec::new(map_rejection, decode_rejection);
        executor
            .execute_with_rejection_codec(&handler, metadata, &command, &codec)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await?;

    // Controlled destruction of this fixture's owned authority, not an operationally
    // supported rolling upgrade. A new winner must not hide the observed incarnation loss.
    let original = fixture.js.get_stream(store.config().stream_name()).await?;
    assert_eq!(original.cached_info().state.messages, 0);
    let config = original.cached_info().config.clone();
    fixture
        .js
        .delete_stream(store.config().stream_name())
        .await?;
    let recreated = fixture.js.create_stream(config).await?;
    assert_ne!(
        original.cached_info().created,
        recreated.cached_info().created
    );
    let fresh_store =
        Arc::new(NatsEventStore::connect(fixture.js.clone(), store.config().clone()).await?);
    let fresh_processor = processor(&fixture.context, fresh_store.clone(), calls.clone(), None)?;
    if accepted {
        local_bus(&fixture.context, fresh_processor.clone())
            .dispatch(request("session-repair", "target", "create")?)
            .await?;
    }
    let response = fresh_processor.process(&encoded).await?;
    assert_eq!(
        matches!(response.outcome(), CommandResponseOutcome::Accepted),
        accepted
    );
    assert_eq!(rejected(response.outcome()), !accepted);
    let before = fresh_store
        .load_completed_outcome_in_context(fixture.context.name(), encoded.operation_id())
        .await?
        .ok_or("missing fresh winner")?;
    let before_events = fresh_store.load(&stream("target")?).await?;
    release.add_permits(1);
    let result = task.await?;
    println!(
        "recreated owned authority: old={:?}, new={:?}, accepted_winner={accepted}, old_attempt={result:?}",
        original.cached_info().created,
        recreated.cached_info().created
    );
    let error = result.expect_err("a concurrent winner must not bypass stale-session validation");
    assert!(
        matches!(&error, CommandExecutionError::RejectedOutcomePersistence(error)
        if error.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert_eq!(
        error.execution_failure(),
        Some(ExecutionFailure::Store(
            StoreFailureKind::ConfigurationMismatch
        ))
    );
    assert_eq!(
        fresh_store
            .load_completed_outcome_in_context(fixture.context.name(), encoded.operation_id())
            .await?
            .unwrap(),
        before
    );
    assert_eq!(fresh_store.load(&stream("target")?).await?, before_events);
    assert_eq!(calls.load(Ordering::SeqCst), if accepted { 3 } else { 2 });
    Ok(())
}

#[tokio::test]
async fn accepted_winner_cannot_bypass_rejected_session_incarnation() -> TestResult {
    recreated_session_winner(true).await
}

#[tokio::test]
async fn rejected_winner_cannot_bypass_rejected_session_incarnation() -> TestResult {
    recreated_session_winner(false).await
}
