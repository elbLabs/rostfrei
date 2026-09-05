//! Independent confirmation of the repaired core boundary beneath service projection.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{AllowAllCommands, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

#[derive(Clone, Copy)]
enum ReturnPath {
    InnerLookup,
    ExactCommit,
    Reconciliation,
}
struct Backend {
    local: InMemoryEventStore,
    foreign: CompletedOutcome,
    path: ReturnPath,
    lookups: AtomicUsize,
    committed: AtomicBool,
    begins: AtomicUsize,
}
#[async_trait]
impl EventHistory for Backend {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.local.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for Backend {
    async fn lookup_outcome(
        &self,
        _: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        let count = self.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(
            if (matches!(self.path, ReturnPath::InnerLookup) && count > 0)
                || self.committed.load(Ordering::SeqCst)
            {
                OutcomeLookup::Completed(self.foreign.clone())
            } else {
                OutcomeLookup::Missing
            },
        )
    }
}
#[async_trait]
impl ExecutionStore for Backend {
    fn authority(&self) -> &CommandAuthority {
        self.local.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.begins.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Session(self)))
    }
}
struct Session<'a>(&'a Backend);
#[async_trait]
impl EventHistory for Session<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.local.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for Session<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        _: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.0.committed.store(true, Ordering::SeqCst);
        if matches!(self.0.path, ReturnPath::ExactCommit) {
            Ok(CompletedCommit::ExactReplay(self.0.foreign.clone()))
        } else {
            Err(EventStoreError::new(
                EventStoreErrorKind::Conflict,
                "winner race",
            ))
        }
    }
}

#[tokio::test]
async fn service_inner_lookup_exact_commit_and_reconciled_foreign_winners_fail_before_projection()
-> TestResult {
    let who =
        TrustedCommandContext::new(PrincipalId::new("service")?, SecurityScope::new("scope")?);
    let command = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(request("review-fixed", true)?)?;
    let foreign = Arc::new(InMemoryEventStore::new());
    let mut original = CommandProcessor::builder(context()?, foreign.clone())
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    original.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    CommandProcessingService::process(&original, &who, &command)
        .await
        .into_result()?;
    let key = OutcomeKey::new(
        foreign.authority().clone(),
        context()?.name().clone(),
        OperationId::new(command.operation_id().as_str())?,
    );
    let OutcomeLookup::Completed(original) = foreign.lookup_outcome(&key).await? else {
        return Err("missing control".into());
    };
    let states = vec![
        original.details().clone(),
        OutcomeDetails::Expired,
        OutcomeDetails::Archived {
            reference: ArchiveReference::new("archive:foreign")?,
        },
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::MissingDetails,
        },
        OutcomeDetails::Unavailable {
            reason: DetailUnavailableReason::BackendUnavailable,
        },
    ];
    for state in states {
        for path in [
            ReturnPath::InnerLookup,
            ReturnPath::ExactCommit,
            ReturnPath::Reconciliation,
        ] {
            let backend = Arc::new(Backend {
                local: InMemoryEventStore::new(),
                foreign: CompletedOutcome::new(original.marker().clone(), state.clone())?,
                path,
                lookups: AtomicUsize::new(0),
                committed: AtomicBool::new(false),
                begins: AtomicUsize::new(0),
            });
            let calls = Arc::new(AtomicUsize::new(0));
            let mut processor = CommandProcessor::builder(context()?, backend.clone())
                .authorizer(Arc::new(AllowAllCommands))
                .conflict_retry_policy(Arc::new(NoConflictRetries))
                .build()?;
            processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
            let error = CommandProcessingService::process(&processor, &who, &command)
                .await
                .into_result()
                .unwrap_err();
            // SO2 tracks precise origin classification separately; neither allowed kind is business success.
            assert!(matches!(
                error.kind(),
                rostfrei::CommandProcessorErrorKind::CompletedDetailsBlocked
                    | rostfrei::CommandProcessorErrorKind::ExecutionFailed
            ));
            let expected = usize::from(!matches!(path, ReturnPath::InnerLookup));
            assert_eq!(calls.load(Ordering::SeqCst), expected);
            assert_eq!(backend.begins.load(Ordering::SeqCst), expected);
            assert!(backend.local.load(&stream()?).await?.is_empty());
        }
    }
    Ok(())
}
