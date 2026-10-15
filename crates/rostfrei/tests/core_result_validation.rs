//! Core boundary failures must survive the service/compatibility projections.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{AllowAllCommands, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

struct ForeignBackend {
    local: InMemoryEventStore,
    outcome: CompletedOutcome,
    at_commit: bool,
    begins: AtomicUsize,
}
#[async_trait]
impl EventHistory for ForeignBackend {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.local.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for ForeignBackend {
    async fn lookup_outcome(
        &self,
        _: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        Ok(if self.at_commit {
            OutcomeLookup::Missing
        } else {
            OutcomeLookup::Completed(self.outcome.clone())
        })
    }
}
#[async_trait]
impl ExecutionStore for ForeignBackend {
    fn authority(&self) -> &CommandAuthority {
        self.local.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(ForeignSession(self)))
    }
}
struct ForeignSession<'a>(&'a ForeignBackend);
#[async_trait]
impl EventHistory for ForeignSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.local.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for ForeignSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        _: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        Ok(CompletedCommit::Appended(self.0.outcome.clone()))
    }
}

#[tokio::test]
async fn service_and_compatibility_paths_never_expose_foreign_authority_results() -> TestResult {
    for trusted in [false, true] {
        let foreign = Arc::new(InMemoryEventStore::new());
        let who =
            TrustedCommandContext::new(PrincipalId::new("service")?, SecurityScope::new("scope")?);
        let mut original = if trusted {
            CommandProcessor::new(context()?.name().clone(), foreign.clone())
        } else {
            CommandProcessor::builder(context()?, foreign.clone())
                .authorizer(Arc::new(AllowAllCommands))
                .build()?
        };
        original
            .register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
        let command = bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)?
        .encode(request("foreign-result", true)?)?;
        if trusted {
            original.process(&command).await?;
        } else {
            CommandProcessingService::process(&original, &who, &command)
                .await
                .into_result()?;
        }
        let key = OutcomeKey::new(
            foreign.authority().clone(),
            context()?.name().clone(),
            OperationId::new(command.operation_id().as_str())?,
        );
        let OutcomeLookup::Completed(outcome) = foreign.lookup_outcome(&key).await? else {
            return Err("missing foreign control".into());
        };
        for at_commit in [false, true] {
            let backend = Arc::new(ForeignBackend {
                local: InMemoryEventStore::new(),
                outcome: outcome.clone(),
                at_commit,
                begins: AtomicUsize::new(0),
            });
            let calls = Arc::new(AtomicUsize::new(0));
            let mut processor = if trusted {
                CommandProcessor::new(context()?.name().clone(), backend.clone())
            } else {
                CommandProcessor::builder(context()?, backend.clone())
                    .authorizer(Arc::new(AllowAllCommands))
                    .build()?
            };
            processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
            if trusted {
                assert!(processor.process(&command).await.is_err());
                if !at_commit {
                    assert!(processor.reconcile_completed(&command).await.is_err());
                }
            } else {
                let error = CommandProcessingService::process(&processor, &who, &command)
                    .await
                    .into_result()
                    .unwrap_err();
                assert!(matches!(
                    error.kind(),
                    rostfrei::CommandProcessorErrorKind::CompletedDetailsBlocked
                        | rostfrei::CommandProcessorErrorKind::ExecutionFailed
                ));
                if !at_commit {
                    assert!(
                        processor
                            .lookup(&who, &command)
                            .await
                            .into_result()
                            .is_err()
                    );
                }
            }
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(at_commit));
            assert_eq!(
                backend.begins.load(Ordering::SeqCst),
                usize::from(at_commit)
            );
            assert!(backend.local.load(&stream()?).await?.is_empty());
        }
    }
    Ok(())
}
