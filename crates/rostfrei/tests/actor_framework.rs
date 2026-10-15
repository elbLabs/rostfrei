#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{
    AllowAllCommands, CommandProcessingService, CompletionOrigin, DynamicCommandRequest,
    PrincipalId, TrustedCommandContext,
};
rostfrei::install_macro_support!();

struct HistoryOnly(Vec<RecordedEvent>);
#[async_trait]
impl EventHistory for HistoryOnly {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Ok(self.0.clone())
    }
}
struct SimulateClaim(ActorRef);
#[async_trait]
impl CommandHandler<()> for SimulateClaim {
    type Rejection = String;
    async fn handle(
        &self,
        (): &(),
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        assert_eq!(execution.metadata().actor(), Some(&self.0));
        let mut counter = execution.load::<Counter>("one").await?;
        counter.raise(Added(7));
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::test]
async fn simulation_preserves_claim_and_history_rejects_intra_commit_actor_drift() -> TestResult {
    let alice = ActorRef::new("people", "alice")?;
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("simulation")?,
        ContentFingerprint::digest("same"),
    )
    .with_actor(alice.clone());
    let outcome = CommandExecutor::new(HistoryOnly(Vec::new()))
        .simulate(&SimulateClaim(alice.clone()), metadata.clone(), &())
        .await?;
    assert_eq!(outcome.participants().len(), 1);
    assert_eq!(
        outcome
            .participants()
            .first()
            .ok_or("missing simulation")?
            .events()
            .len(),
        1
    );
    let commit = derive_commit_id(&stream()?, metadata.operation_id());
    for second in [
        None,
        Some(ActorRef::new("people", "bob")?),
        Some(alice.clone()),
    ] {
        let mut events = Vec::new();
        for ordinal in 0..2 {
            let mut event = RecordedEvent::new_in_commit(
                stream()?,
                StreamVersion::new(u64::from(ordinal).saturating_add(1)),
                derive_event_id(&commit, ordinal),
                commit.clone(),
                metadata.operation_id().clone(),
                metadata.operation_fingerprint(),
                ordinal,
                2,
                "added",
                2,
                b"1".to_vec(),
            )?;
            if let Some(actor) = if ordinal == 0 {
                Some(&alice)
            } else {
                second.as_ref()
            } {
                event = event.with_actor(actor.clone());
            }
            events.push(event);
        }
        let result = CommandExecutor::new(HistoryOnly(events))
            .rehydrate::<Counter>(&stream()?)
            .await;
        if second.as_ref() == Some(&alice) {
            assert_eq!(*result?.state(), 2);
        } else {
            assert!(
                matches!(result, Err(SimulationError::Store(e)) if e.kind() == EventStoreErrorKind::CorruptHistory)
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn attributed_framework_refusal_is_durable_without_bindings_or_details() -> TestResult {
    let actor = ActorRef::new("people", "alice")?;
    for (name, version, payload) in [
        ("unregistered", 1, serde_json::json!({})),
        ("change", 99, serde_json::json!({})),
        (
            "change",
            1,
            serde_json::json!({"amount":"invalid", "reject":false}),
        ),
    ] {
        let store = Arc::new(InMemoryEventStore::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut service = CommandProcessor::builder(context()?, store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .build()?;
        service.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
        let command = bus(baseline_processor(store.clone(), calls.clone())?)?.encode_dynamic(
            DynamicCommandRequest::new(OperationId::new("framework")?, name, version, payload)?
                .with_actor(actor.clone()),
        )?;
        let caller =
            TrustedCommandContext::new(PrincipalId::new("worker")?, SecurityScope::new("scope")?);
        let first = CommandProcessingService::process(&service, &caller, &command)
            .await
            .into_result()?;
        assert_eq!(first.origin(), CompletionOrigin::Committed);
        assert_eq!(
            first.outcome().marker().decision(),
            OutcomeDecision::Rejected
        );
        assert_eq!(first.outcome().marker().actor(), Some(&actor));
        assert!(matches!(
            first.outcome().details().available()?.outcome(),
            CommandResponseOutcome::Rejected(_)
        ));
        let binding_free = CommandProcessor::builder(context()?, store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .build()?;
        let replay = CommandProcessingService::process(&binding_free, &caller, &command)
            .await
            .into_result()?;
        assert_eq!(replay.origin(), CompletionOrigin::Replayed);
        assert_eq!(replay.outcome(), first.outcome());
        let marker = first.outcome().marker();
        store
            .expire_details(marker.detail_reference().key(), marker.revision())
            .await?;
        let replay = CommandProcessingService::process(&binding_free, &caller, &command)
            .await
            .into_result()?;
        assert_eq!(replay.origin(), CompletionOrigin::Replayed);
        assert_eq!(replay.outcome().marker(), marker);
        assert!(matches!(
            replay.outcome().details(),
            OutcomeDetails::Expired
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(store.load(&stream()?).await?.is_empty());
    }
    Ok(())
}
