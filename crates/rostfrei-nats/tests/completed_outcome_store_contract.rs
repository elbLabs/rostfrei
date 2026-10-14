#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
rostfrei::install_macro_support!();

fn decode_domain_rejection(rejection: &CommandRejection) -> Result<&'static str, String> {
    if rejection.code().as_str() == "contract.target-not-found" {
        Ok("target-not-found")
    } else {
        Err("unknown stored domain rejection code".to_owned())
    }
}

async fn generic_processor_interop(direct_first: bool) -> TestResult {
    let fixture = Fixture::new("92-generic-processor-shared-authority").await?;
    let store = fixture.event_store().await?;
    fixture
        .js
        .delete_stream(fixture.messaging.topology().command_stream().as_str())
        .await?;
    fixture
        .js
        .delete_stream(
            fixture
                .messaging
                .topology()
                .command_response_stream()
                .as_str(),
        )
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, processor.clone());
    let encoded = local.encode(request("generic-interop-rejected", "target", "add")?)?;
    let command = Act {
        target: "target".to_owned(),
        action: "add".to_owned(),
        item: "I".to_owned(),
    };
    let codec = rostfrei::MappedCommandRejectionCodec::new(map_rejection, decode_domain_rejection);
    let executor = CommandExecutor::new(store.clone());
    if direct_first {
        assert!(matches!(
            executor
                .execute_with_rejection_codec(
                    &Handler(calls.clone()),
                    encoded.validated_execution_metadata()?,
                    &command,
                    &codec
                )
                .await?,
            CommandOutcome::Rejected("target-not-found")
        ));
    } else {
        assert!(rejected(processor.process(&encoded).await?.outcome()));
    }
    let response = processor.process(&encoded).await?;
    assert!(rejected(response.outcome()));
    assert!(matches!(
        executor
            .execute_with_rejection_codec(
                &Handler(calls.clone()),
                encoded.validated_execution_metadata()?,
                &command,
                &codec
            )
            .await?,
        CommandOutcome::Rejected("target-not-found")
    ));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "both typed surfaces must decode one shared result without another handler call"
    );
    local
        .dispatch(request("generic-interop-repair", "target", "create")?)
        .await?;
    assert_eq!(processor.process(&encoded).await?, response);
    assert!(matches!(
        executor
            .execute_with_rejection_codec(
                &Handler(calls.clone()),
                encoded.validated_execution_metadata()?,
                &command,
                &codec
            )
            .await?,
        CommandOutcome::Rejected("target-not-found")
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn generic_domain_rejection_replays_in_processor_with_explicit_mapping_codec() -> TestResult {
    generic_processor_interop(true).await
}
#[tokio::test]
async fn processor_rejection_replays_in_generic_executor_with_explicit_mapping_codec() -> TestResult
{
    generic_processor_interop(false).await
}

#[tokio::test]
async fn real_nats_completed_outcome_store_and_session_conformance() -> TestResult {
    let fixture = Fixture::new("92-store-conformance").await?;
    let store = fixture.event_store().await?;
    rostfrei_testing::completed_outcome_contract::run(store.as_ref(), fixture.context.name()).await
}

struct CompetingDecision {
    reject: bool,
    barrier: Arc<tokio::sync::Barrier>,
}
#[async_trait]
impl CommandHandler<Act> for CompetingDecision {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut target = execution.load::<Target>(&command.target).await?;
        self.barrier.wait().await;
        if self.reject {
            Ok(CommandDecision::Rejected("target-not-found"))
        } else {
            target.raise(Changed {
                created: true,
                item: Some("I".to_owned()),
            });
            Ok(CommandDecision::Accepted)
        }
    }
}

#[tokio::test]
async fn concurrent_direct_accept_and_native_reject_have_one_durable_decision_and_effect()
-> TestResult {
    let fixture = Fixture::new("92-mixed-competing-decisions").await?;
    let store = fixture.event_store().await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut accept = CommandProcessor::new(fixture.context.name().clone(), store.clone());
    accept.register::<Act, _>(
        CompetingDecision {
            reject: false,
            barrier: barrier.clone(),
        },
        map_rejection,
    )?;
    let mut reject = CommandProcessor::new(fixture.context.name().clone(), store.clone());
    reject.register::<Act, _>(
        CompetingDecision {
            reject: true,
            barrier,
        },
        map_rejection,
    )?;
    let _worker = fixture.command_worker(Arc::new(reject)).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let command = remote.encode(request("mixed-decision-operation", "target", "create")?)?;
    let (a, b) = tokio::time::timeout(WAIT, async {
        tokio::join!(
            accept.process(&command),
            remote.dispatch_encoded(command.clone())
        )
    })
    .await?;
    let (a, b) = (a?, b?);
    assert_eq!(&a, b.response());
    let history = store.load(&stream("target")?).await?;
    assert_eq!(
        history.len(),
        usize::from(a.outcome() == &CommandResponseOutcome::Accepted)
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let fresh = processor(&fixture.context, store, calls.clone(), None)?;
    assert_eq!(fresh.process(&command).await?, a);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn direct_nats_store_rejects_and_replays_without_any_command_or_response_stream() -> TestResult
{
    let fixture = Fixture::new("92-direct-no-command-queue").await?;
    let store = fixture.event_store().await?;
    fixture
        .js
        .delete_stream(fixture.messaging.topology().command_stream().as_str())
        .await?;
    fixture
        .js
        .delete_stream(
            fixture
                .messaging
                .topology()
                .command_response_stream()
                .as_str(),
        )
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let first = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let bus = local_bus(&fixture.context, first.clone());
    let command = bus.encode(request("direct-rejection", "target", "add")?)?;
    let response = first.process(&command).await?;
    assert!(rejected(response.outcome()));
    bus.dispatch(request("direct-repair", "target", "create")?)
        .await?;
    let recreated = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    assert_eq!(recreated.process(&command).await?, response);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

struct RequestRefusalAfterRead {
    loaded: Arc<tokio::sync::Semaphore>,
    resume: Arc<tokio::sync::Semaphore>,
    first: AtomicBool,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl CommandHandler<Act> for RequestRefusalAfterRead {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _target = execution.load::<Target>(&command.target).await?;
        if self.first.swap(false, Ordering::SeqCst) {
            self.loaded.add_permits(1);
            self.resume
                .acquire()
                .await
                .map_err(|error| {
                    EventStoreError::new(EventStoreErrorKind::Unavailable, error.to_string())
                })?
                .forget();
        }
        Err(EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            "controlled request refusal after successful read",
        )
        .into())
    }
}

#[tokio::test]
async fn framework_request_rejection_keeps_the_original_missing_read_guard() -> TestResult {
    let fixture = Fixture::new("92-request-guard").await?;
    let store = fixture.event_store().await?;
    let loaded = Arc::new(tokio::sync::Semaphore::new(0));
    let resume = Arc::new(tokio::sync::Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut guarded = CommandProcessor::new(fixture.context.name().clone(), store.clone());
    guarded.register::<Act, _>(
        RequestRefusalAfterRead {
            loaded: loaded.clone(),
            resume: resume.clone(),
            first: AtomicBool::new(true),
            calls: calls.clone(),
        },
        map_rejection,
    )?;
    let ordinary = processor(
        &fixture.context,
        store.clone(),
        Arc::new(AtomicUsize::new(0)),
        None,
    )?;
    let local = local_bus(&fixture.context, ordinary);
    let command = local.encode(request("request-refusal", "target", "add")?)?;
    let submission = tokio::spawn(async move { guarded.process(&command).await });
    tokio::time::timeout(WAIT, loaded.acquire())
        .await??
        .forget();
    local
        .dispatch(request("request-refusal-repair", "target", "create")?)
        .await?;
    resume.add_permits(1);
    let response = tokio::time::timeout(WAIT, submission).await???;
    assert!(
        matches!(response.outcome(), CommandResponseOutcome::Rejected(rejection) if rejection.classification() == CommandRejectionClassification::InvalidRequest)
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "stale missing read must rerun the original request refusal"
    );
    let record = store
        .load_completed_outcome_in_context(
            fixture.context.name(),
            &OperationId::new("request-refusal")?,
        )
        .await?
        .ok_or("missing request outcome")?;
    assert_eq!(record.record().streams().len(), 1);
    assert_eq!(
        record
            .record()
            .streams()
            .first()
            .ok_or("missing touched stream")?
            .base_version(),
        StreamVersion::new(1)
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}
