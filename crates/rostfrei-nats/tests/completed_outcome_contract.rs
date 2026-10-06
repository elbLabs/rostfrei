//! #92: completed business outcomes are shared below adapters, not adapter-local cache policy.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei_runtime::{CommandBusObserver, CommandBusReceipt, CommandPublication};

struct ConcurrencyGate {
    published: tokio::sync::Semaphore,
    loaded: tokio::sync::Semaphore,
    resume: tokio::sync::Semaphore,
}
#[async_trait]
impl CommandBusObserver for ConcurrencyGate {
    async fn published(&self, _: CommandPublication) {
        self.published.add_permits(1);
    }
}

struct ConcurrentHandler {
    gate: Arc<ConcurrencyGate>,
    first: AtomicBool,
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl CommandHandler<Act> for ConcurrentHandler {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        // Keep one execution pending until both adapters have published/submitted.
        // Do not require a second handler invocation: shared serialization is valid.
        let mut target = execution.load::<Target>(&command.target).await?;
        if self.first.swap(false, Ordering::SeqCst) {
            self.gate.loaded.add_permits(1);
            self.gate
                .resume
                .acquire()
                .await
                .map_err(|e| EventStoreError::new(EventStoreErrorKind::Unavailable, e.to_string()))?
                .forget();
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        if command.action == "add" && !target.aggregate().state().exists {
            return Ok(CommandDecision::Rejected("target-not-found"));
        }
        target.aggregate_mut().raise(Changed {
            created: command.action == "create",
            item: (command.action == "add").then(|| command.item.clone()),
        });
        Ok(CommandDecision::Accepted)
    }
}
fn concurrent_processor(
    fixture: &Fixture,
    store: Arc<dyn ExecutionStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<(Arc<CommandProcessor>, Arc<ConcurrencyGate>)> {
    let gate = Arc::new(ConcurrencyGate {
        published: tokio::sync::Semaphore::new(0),
        loaded: tokio::sync::Semaphore::new(0),
        resume: tokio::sync::Semaphore::new(0),
    });
    let mut processor = CommandProcessor::new(fixture.context.name().clone(), store);
    processor.register::<Act, _>(
        ConcurrentHandler {
            gate: gate.clone(),
            first: AtomicBool::new(true),
            calls,
        },
        map_rejection,
    )?;
    Ok((Arc::new(processor), gate))
}
async fn concurrent_submissions(
    local: &CommandBus,
    remote: &CommandBus,
    submission: CommandRequest<Act>,
    gate: Arc<ConcurrencyGate>,
) -> TestResult<(CommandBusReceipt, CommandBusReceipt)> {
    let release = async {
        gate.published.acquire_many(2).await?.forget();
        gate.loaded.acquire().await?.forget();
        gate.resume.add_permits(1);
        TestResult::Ok(())
    };
    let (left, right, release) = tokio::time::timeout(WAIT, async {
        tokio::join!(
            local.dispatch_observed(submission.clone(), gate.clone()),
            remote.dispatch_observed(submission, gate.clone()),
            release
        )
    })
    .await?;
    release?;
    Ok((left?, right?))
}

async fn rejection_replay(first_remote: bool, first_local: bool) -> TestResult {
    let fixture = Fixture::new("92-rejection").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, processor.clone());
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let _worker = fixture.command_worker(processor.clone()).await?;
    let submission = request("rejected-operation", "target", "add")?;
    let encoded = local.encode(submission.clone())?;
    assert_eq!(
        encoded,
        remote.encode(submission.clone())?,
        "all canonical evidence, context, correlation, causation and timestamp must match byte-for-byte"
    );
    println!(
        "canonical_command={}",
        String::from_utf8_lossy(encoded.payload())
    );
    let first = if first_remote {
        remote.dispatch(submission.clone()).await?.into_response()
    } else if first_local {
        local.dispatch(submission.clone()).await?.into_response()
    } else {
        processor.process(&encoded).await?
    };
    assert!(rejected(first.outcome()));
    assert!(store.load(&stream("target")?).await?.is_empty());
    local
        .dispatch(request("create-prerequisite", "target", "create")?)
        .await?;
    let before = calls.load(Ordering::SeqCst);
    // Recreate both processor and adapter to rule out an in-memory rejection cache.
    let recreated =
        release_contract_support::processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let replay = if first_remote {
        recreated.process(&encoded).await?
    } else {
        remote.dispatch(submission.clone()).await?.into_response()
    };
    assert_eq!(
        replay, first,
        "same completed rejection must remain authoritative after prerequisite repair"
    );
    assert_eq!(
        local_bus(&fixture.context, recreated.clone())
            .dispatch(submission.clone())
            .await?
            .response(),
        &first
    );
    assert_eq!(remote.dispatch(submission).await?.response(), &first);
    assert_eq!(
        store.load(&stream("target")?).await?.len(),
        1,
        "rejected operation must not mutate repaired target"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before,
        "completed outcome replay must not reevaluate business logic"
    );
    assert_eq!(
        local_bus(&fixture.context, recreated)
            .dispatch(request("new-attempt", "target", "add")?)
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    let state = CommandExecutor::new(store.clone())
        .rehydrate::<Target>(&stream("target")?)
        .await?;
    assert_eq!(state.state().items, ["I"]);
    assert_eq!(store.load(&stream("target")?).await?.len(), 2);
    Ok(())
}
#[tokio::test]
async fn nats_rejection_replays_through_direct_and_fresh_local_processor() -> TestResult {
    rejection_replay(true, false).await
}
#[tokio::test]
async fn direct_rejection_replays_through_nats_after_prerequisite_repair() -> TestResult {
    rejection_replay(false, false).await
}
#[tokio::test]
async fn local_rejection_replays_through_nats_after_prerequisite_repair() -> TestResult {
    rejection_replay(false, true).await
}

async fn changed_evidence(variant: u8) -> TestResult {
    let fixture = Fixture::new("92-evidence").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, processor.clone());
    let rejected_request = request("identity", "target", "add")?;
    assert!(rejected(
        processor
            .process(&local.encode(rejected_request.clone())?)
            .await?
            .outcome()
    ));
    local
        .dispatch(request("prerequisite", "target", "create")?)
        .await?;
    let altered = match variant {
        0 => {
            let command = Act {
                target: "target".to_owned(),
                action: "add".to_owned(),
                item: "different-item".to_owned(),
            };
            CommandRequest::new(OperationId::new("identity")?, command)
                .with_correlation_id(CorrelationId::new("contract-correlation")?)
                .with_causation_id(CausationId::new("contract-causation")?)
                .with_created_at(MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?)
        }
        1 => rejected_request.with_correlation_id(CorrelationId::new("different-correlation")?),
        _ => rejected_request.with_causation_id(CausationId::new("different-causation")?),
    };
    let before = calls.load(Ordering::SeqCst);
    let _worker = fixture.command_worker(processor).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let receipt = remote.dispatch(altered).await?;
    let CommandResponseOutcome::Rejected(error) = receipt.response().outcome() else {
        return Err("recorded rejected operation accepted changed evidence".into());
    };
    assert_eq!(
        error.classification(),
        CommandRejectionClassification::Conflict
    );
    assert_eq!(
        error.code().as_str(),
        "rostfrei.operation.identity-conflict"
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}
#[tokio::test]
async fn changed_payload_cannot_reuse_a_completed_rejected_identity() -> TestResult {
    changed_evidence(0).await
}
#[tokio::test]
async fn changed_correlation_cannot_reuse_a_completed_rejected_identity() -> TestResult {
    changed_evidence(1).await
}
#[tokio::test]
async fn changed_causation_cannot_reuse_a_completed_rejected_identity() -> TestResult {
    changed_evidence(2).await
}

#[tokio::test]
async fn concurrent_local_and_nats_rejections_have_one_stable_outcome() -> TestResult {
    let fixture = Fixture::new("92-concurrent-reject").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let (processor, gate) = concurrent_processor(&fixture, store.clone(), calls.clone())?;
    let local = local_bus(&fixture.context, processor.clone());
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let _worker = fixture.command_worker(processor).await?;
    let submission = request("concurrent", "target", "add")?;
    assert_eq!(
        local.encode(submission.clone())?,
        remote.encode(submission.clone())?
    );
    let (left, right) = concurrent_submissions(&local, &remote, submission.clone(), gate).await?;
    assert!(rejected(left.response().outcome()));
    assert_eq!(left.response(), right.response());
    assert!(store.load(&stream("target")?).await?.is_empty());
    local
        .dispatch(request("prerequisite", "target", "create")?)
        .await?;
    let fresh = local_bus(
        &fixture.context,
        release_contract_support::processor(&fixture.context, store.clone(), calls, None)?,
    );
    assert_eq!(
        fresh.dispatch(submission).await?.response(),
        left.response(),
        "concurrent completed rejection must survive adapter recreation and state repair"
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn concurrent_local_and_nats_acceptance_commits_at_most_one_effect() -> TestResult {
    let fixture = Fixture::new("92-concurrent-accept").await?;
    let store = fixture.split_event_store().await?;
    let (processor, gate) =
        concurrent_processor(&fixture, store.clone(), Arc::new(AtomicUsize::new(0)))?;
    let local = local_bus(&fixture.context, processor.clone());
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let _worker = fixture.command_worker(processor).await?;
    let submission = request("concurrent", "target", "create")?;
    let (left, right) = concurrent_submissions(&local, &remote, submission, gate).await?;
    assert_eq!(left.response().outcome(), &CommandResponseOutcome::Accepted);
    assert_eq!(left.response(), right.response());
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn lost_accepted_response_replays_across_adapters_without_second_commit() -> TestResult {
    let fixture = Fixture::new("92-lost-response").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let original = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, original.clone());
    let submission = request("lost-response", "target", "create")?;
    let encoded = local.encode(submission.clone())?;
    // Commit succeeds, but the caller never receives this direct response.
    drop(original.process(&encoded).await?);
    drop(original);
    drop(local);
    let before = calls.load(Ordering::SeqCst);
    let recreated = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let _worker = fixture.command_worker(recreated.clone()).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert_eq!(remote.encode(submission.clone())?, encoded);
    assert_eq!(
        remote
            .dispatch(submission.clone())
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(
        local_bus(&fixture.context, recreated)
            .dispatch(submission)
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), before);
    let receipt = store
        .load_transaction_receipt_in_context(
            fixture.context.name(),
            &OperationId::new("lost-response")?,
        )
        .await?
        .ok_or("missing accepted receipt")?;
    assert_eq!(receipt.operation_fingerprint(), encoded.fingerprint());
    assert_eq!(
        receipt.correlation_id().map(CorrelationId::as_str),
        Some("contract-correlation")
    );
    assert_eq!(
        receipt.causation_id().map(CausationId::as_str),
        Some("contract-causation")
    );
    Ok(())
}

#[tokio::test]
async fn accepted_no_event_receipt_replays_below_local_and_nats_adapters() -> TestResult {
    let fixture = Fixture::new("92-noevent").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, processor.clone());
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let _worker = fixture.command_worker(processor.clone()).await?;
    let submission = request("no-event", "unused", "noop")?;
    assert_eq!(
        remote
            .dispatch(submission.clone())
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    let receipt = store
        .load_transaction_receipt_in_context(fixture.context.name(), &OperationId::new("no-event")?)
        .await?
        .ok_or("missing accepted no-event receipt")?;
    assert!(receipt.events().is_empty());
    assert!(receipt.streams().is_empty());
    let before = calls.load(Ordering::SeqCst);
    let encoded = local.encode(submission.clone())?;
    assert_eq!(
        processor.process(&encoded).await?.outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(
        local
            .dispatch(submission.clone())
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(
        remote.dispatch(submission).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert!(store.load(&stream("unused")?).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn technical_failure_does_not_cache_a_completed_business_rejection() -> TestResult {
    let fixture = Fixture::new("92-technical").await?;
    let store = Arc::new(FaultStore {
        inner: InMemoryEventStore::new(),
        kind: EventStoreErrorKind::Unavailable,
        active: Arc::new(AtomicBool::new(true)),
    });
    let processor = processor(
        &fixture.context,
        store.clone(),
        Arc::new(AtomicUsize::new(0)),
        None,
    )?;
    let local = local_bus(&fixture.context, processor.clone());
    let submission = request("technical", "bad-target", "create")?;
    assert_eq!(
        local.dispatch(submission.clone()).await.unwrap_err().kind(),
        rostfrei_runtime::CommandBusErrorKind::Unavailable
    );
    assert!(
        store
            .inner
            .load_transaction_receipt_in_context(
                fixture.context.name(),
                &OperationId::new("technical")?
            )
            .await?
            .is_none()
    );
    store.active.store(false, Ordering::SeqCst);
    let _worker = fixture.command_worker(processor).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert_eq!(
        remote.dispatch(submission).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(store.inner.load(&stream("bad-target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn fresh_operation_after_rejection_and_prerequisite_repair_can_succeed() -> TestResult {
    let fixture = Fixture::new("92-fresh-attempt").await?;
    let store = fixture.split_event_store().await?;
    let processor = processor(
        &fixture.context,
        store.clone(),
        Arc::new(AtomicUsize::new(0)),
        None,
    )?;
    let local = local_bus(&fixture.context, processor.clone());
    assert!(rejected(
        local
            .dispatch(request("rejected", "target", "add")?)
            .await?
            .response()
            .outcome()
    ));
    local
        .dispatch(request("prerequisite", "target", "create")?)
        .await?;
    let _worker = fixture.command_worker(processor).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert_eq!(
        remote
            .dispatch(request("fresh-attempt", "target", "add")?)
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    let state = CommandExecutor::new(store.clone())
        .rehydrate::<Target>(&stream("target")?)
        .await?;
    assert_eq!(state.state().items, ["I"]);
    assert_eq!(store.load(&stream("target")?).await?.len(), 2);
    Ok(())
}

async fn exhausted_outcome_store() -> TestResult<(
    Fixture,
    Arc<NatsEventStore>,
    Arc<CommandProcessor>,
    CommandBus,
)> {
    let fixture = Fixture::new("92-full-outcome-store").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&fixture.context)?
        .with_fresh_split_outcome_epoch("private-capacity-fixture")?
        .with_storage_limits(8192, 2048)?;
    provision_event_store(&fixture.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(fixture.js.clone(), config).await?);
    let mut exhausted = false;
    for index in 0..100 {
        let transaction = EventTransaction::new(
            OperationId::new(format!("fill-{index}"))?,
            ContentFingerprint::digest("fill"),
            Vec::new(),
        )
        .with_bounded_context(fixture.context.name().clone());
        if let Err(error) = store.append_transaction(transaction).await {
            assert_eq!(error.kind(), EventStoreErrorKind::CapacityExhausted);
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "capacity fixture must exercise actual durable append refusal"
    );
    let processor = processor(
        &fixture.context,
        store.clone(),
        Arc::new(AtomicUsize::new(0)),
        None,
    )?;
    let local = local_bus(&fixture.context, processor.clone());
    assert!(
        processor
            .process(&local.encode(request("accept-capacity-control", "unused", "noop")?)?)
            .await
            .is_err(),
        "setup control: even a no-event accepted receipt cannot persist"
    );
    Ok((fixture, store, processor, local))
}
#[tokio::test]
async fn rejection_persistence_failure_is_not_reported_as_completed_by_processor() -> TestResult {
    let (_, store, processor, local) = exhausted_outcome_store().await?;
    let encoded = local.encode(request("failed-rejection", "missing", "add")?)?;
    let result = processor.process(&encoded).await;
    assert!(store.load(&stream("missing")?).await?.is_empty());
    assert!(
        result.is_err(),
        "a full durable outcome sink must not return a completed rejection: {result:?}"
    );
    Ok(())
}
#[tokio::test]
async fn rejection_persistence_failure_does_not_ack_nats_or_publish_completed_response()
-> TestResult {
    let (fixture, _, processor, local) = exhausted_outcome_store().await?;
    let encoded = local.encode(request("failed-rejection", "missing", "add")?)?;
    let disposition = fixture
        .adapter
        .command_handler(processor)
        .handle(command_delivery(&encoded)?)
        .await;
    assert_ne!(
        disposition,
        DeliveryDisposition::Acknowledge,
        "transport must not ACK an unpersisted completed rejection"
    );
    assert_eq!(
        fixture
            .messages(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str()
            )
            .await?,
        0
    );
    Ok(())
}

struct GuardedRejection {
    loaded: Arc<tokio::sync::Semaphore>,
    resume: Arc<tokio::sync::Semaphore>,
    first: AtomicBool,
}
#[async_trait]
impl CommandHandler<Act> for GuardedRejection {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut target = execution.load::<Target>(&command.target).await?;
        let exists = target.aggregate().state().exists;
        if self.first.swap(false, Ordering::SeqCst) {
            self.loaded.add_permits(1);
            self.resume
                .acquire()
                .await
                .map_err(|e| EventStoreError::new(EventStoreErrorKind::Unavailable, e.to_string()))?
                .forget();
        }
        if !exists {
            return Ok(CommandDecision::Rejected("target-not-found"));
        }
        target.aggregate_mut().raise(Changed {
            created: false,
            item: Some(command.item.clone()),
        });
        Ok(CommandDecision::Accepted)
    }
}
#[tokio::test]
async fn rejection_read_guard_is_validated_before_outcome_becomes_authoritative() -> TestResult {
    let fixture = Fixture::new("92-rejection-read-guard").await?;
    let store = fixture.split_event_store().await?;
    let loaded = Arc::new(tokio::sync::Semaphore::new(0));
    let resume = Arc::new(tokio::sync::Semaphore::new(0));
    let mut guarded = CommandProcessor::new(fixture.context.name().clone(), store.clone());
    guarded.register::<Act, _>(
        GuardedRejection {
            loaded: loaded.clone(),
            resume: resume.clone(),
            first: AtomicBool::new(true),
        },
        map_rejection,
    )?;
    let guarded = Arc::new(guarded);
    let bus = local_bus(&fixture.context, guarded.clone());
    let encoded = bus.encode(request("guarded-rejection", "target", "add")?)?;
    let pending = guarded.process(&encoded);
    let state_change = async {
        loaded.acquire().await?.forget();
        local_bus(
            &fixture.context,
            processor(
                &fixture.context,
                store.clone(),
                Arc::new(AtomicUsize::new(0)),
                None,
            )?,
        )
        .dispatch(request(
            "create-while-rejection-pending",
            "target",
            "create",
        )?)
        .await?;
        resume.add_permits(1);
        TestResult::Ok(())
    };
    let (outcome, changed) =
        tokio::time::timeout(WAIT, async { tokio::join!(pending, state_change) }).await?;
    changed?;
    // Like accepted read guards, the stale rejection must conflict/re-evaluate.
    assert_eq!(
        outcome?.outcome(),
        &CommandResponseOutcome::Accepted,
        "a rejection observed before a conflicting committed state change cannot be recorded using a stale guard"
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 2);
    Ok(())
}

rostfrei_runtime::install_macro_support!();
