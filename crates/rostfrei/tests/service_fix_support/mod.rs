//! Fault fixtures for the service-remediation regression targets.
#![allow(dead_code, unused_imports)]
use crate::modular_support::*;
use async_trait::async_trait;
pub use rostfrei::{
    AllowAllCommands, AttemptId, AuditDelivery, AuthorizationError, CommandAccess,
    CommandAuthorizer, CommandProcessingService, CommandProcessorErrorKind, CommandResponse,
    CompletionOrigin, PrincipalId, ProcessedCommand, ServiceReport, TrustedCommandContext,
    ValidatedCommand,
};

pub fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("fix-verification")?,
        SecurityScope::new("scope")?,
    ))
}
pub fn encoded(id: &str, reject: bool) -> TestResult<EncodedCommand> {
    noncanonical(
        &bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)?
        .encode(request(id, reject)?)?,
    )
}
pub fn processor(
    store: Arc<dyn ExecutionStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<CommandProcessor> {
    let mut service = CommandProcessor::builder(context()?, store)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    service.register::<Change, _>(ChangeHandler(calls), map_rejection)?;
    Ok(service)
}
pub struct Substitute(pub ProcessedCommand);
#[async_trait]
impl CommandProcessingService for Substitute {
    async fn process(
        &self,
        _: &TrustedCommandContext,
        _: &EncodedCommand,
    ) -> ServiceReport<ProcessedCommand> {
        ServiceReport::new(
            Ok(self.0.clone()),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
    async fn lookup(
        &self,
        _: &TrustedCommandContext,
        _: &EncodedCommand,
    ) -> ServiceReport<OutcomeLookup<CommandResponse>> {
        ServiceReport::new(
            Ok(OutcomeLookup::Completed(self.0.outcome().clone())),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
}
pub fn opposite_response(original: &CommandResponse) -> TestResult<CommandResponse> {
    Ok(match original.outcome() {
        CommandResponseOutcome::Accepted => CommandResponse::rejected(
            original.message_id().clone(),
            original.command_message_id().clone(),
            original.command_address().clone(),
            original.operation_id().clone(),
            original.correlation_id().clone(),
            map_rejection(&"substituted refusal".to_owned())?,
        )?,
        _ => CommandResponse::accepted(
            original.message_id().clone(),
            original.command_message_id().clone(),
            original.command_address().clone(),
            original.operation_id().clone(),
            original.correlation_id().clone(),
        )?,
    })
}
pub fn substitute_response(
    winner: &ProcessedCommand,
    response: CommandResponse,
) -> TestResult<ProcessedCommand> {
    Ok(ProcessedCommand::new(
        winner.origin(),
        winner
            .outcome()
            .clone()
            .try_map::<_, OutcomeRecoveryError>(|_| Ok(response))?,
    ))
}

pub struct FaultReader(pub DetailReadError);
#[async_trait]
impl OutcomeDetailReader for FaultReader {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        Err(self.0.clone())
    }
}

/// A real different winner is committed only after the contender has evaluated.
pub struct CommitRace {
    pub inner: InMemoryEventStore,
    pub writer: CommandProcessor,
    pub command: EncodedCommand,
    pub who: TrustedCommandContext,
    pub starts: AtomicUsize,
    pub conflicts: AtomicUsize,
}
#[async_trait]
impl EventHistory for CommitRace {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for CommitRace {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl ExecutionStore for CommitRace {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(RaceSession {
            backend: self,
            inner: self.inner.begin_command().await?,
        }))
    }
}
struct RaceSession<'a> {
    backend: &'a CommitRace,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for RaceSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for RaceSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        CommandProcessingService::process(
            &self.backend.writer,
            &self.backend.who,
            &self.backend.command,
        )
        .await
        .into_result()
        .map_err(|e| EventStoreError::new(EventStoreErrorKind::InvalidRequest, e.to_string()))?;
        let result = self.inner.commit_outcome(proposal).await;
        if result
            .as_ref()
            .is_err_and(|e| e.kind() == EventStoreErrorKind::IdentityConflict)
        {
            self.backend.conflicts.fetch_add(1, Ordering::SeqCst);
        }
        result
    }
}
pub struct EventFreeWinner(pub Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<Change> for EventFreeWinner {
    type Rejection = String;
    async fn handle(
        &self,
        _: &Change,
        _: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Accepted)
    }
}
