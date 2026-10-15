//! Independent D2a review controls. No changes to frozen product or authored tests.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{
    AllowAllCommands, AttemptId, AuditDelivery, AuditPhase, AuditTerminal, AuthorizationError,
    CommandAccess, CommandAuditRecord, CommandAuditSink, CommandAuthorizer,
    CommandProcessingService, CompletionOrigin, InMemoryCommandAuditSink, PrincipalId,
    ProcessedCommand, ServiceReport, TrustedCommandContext, ValidatedCommand,
};
rostfrei::install_macro_support!();

fn caller(scope: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("reviewer")?,
        SecurityScope::new(scope)?,
    ))
}
fn encoded(id: &str, reject: bool) -> TestResult<EncodedCommand> {
    noncanonical(
        &bus(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)?
        .encode(request(id, reject)?)?,
    )
}
fn processor(store: Arc<dyn CommandBackend>) -> TestResult<CommandProcessor> {
    let mut service = CommandProcessor::builder(context()?, store)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    service.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    Ok(service)
}

/// Returns an INTACT genuine completion, not a forged marker/detail pair.
struct WrongWinner(ProcessedCommand);
#[async_trait]
impl CommandProcessingService for WrongWinner {
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
    ) -> ServiceReport<OutcomeLookup<rostfrei::CommandResponse>> {
        ServiceReport::new(
            Ok(OutcomeLookup::Completed(self.0.outcome().clone())),
            AttemptId::generate(),
            AuditDelivery::Disabled,
        )
    }
}

#[tokio::test]
async fn local_adapter_refuses_a_genuine_winner_for_another_operation() -> TestResult {
    let service = processor(Arc::new(InMemoryEventStore::new()))?;
    let who = caller("scope-a")?;
    let original = encoded("actual-completed-operation", false)?;
    let requested = encoded("never-executed-operation", false)?;
    let winner = CommandProcessingService::process(&service, &who, &original)
        .await
        .into_result()?;
    assert!(
        requested
            .validate_response(winner.outcome().details().available()?)
            .is_err()
    );
    let adapter = InMemoryMessagingAdapter::with_service(Arc::new(WrongWinner(winner)));
    let result = adapter.dispatch_as(&who, requested).await;
    eprintln!("cross-operation local result: {result:?}");
    assert!(
        result.is_err(),
        "typed service injection admitted another operation's winner"
    );
    assert!(
        adapter.command_messages().await.is_empty(),
        "invalid admission must not enter diagnostics"
    );
    Ok(())
}

#[tokio::test]
async fn local_adapter_refuses_a_genuine_winner_for_another_security_scope() -> TestResult {
    let service = processor(Arc::new(InMemoryEventStore::new()))?;
    let command = encoded("cross-scope-winner", true)?;
    let winner = CommandProcessingService::process(&service, &caller("scope-a")?, &command)
        .await
        .into_result()?;
    let other = caller("scope-b")?;
    let control = CommandProcessingService::process(&service, &other, &command).await;
    assert_eq!(
        control.result().as_ref().unwrap_err().kind(),
        rostfrei::CommandProcessorErrorKind::IdentityConflict
    );
    // Response identity alone matches. The service marker is needed to bind trusted scope.
    command.validate_response(winner.outcome().details().available()?)?;
    let adapter = InMemoryMessagingAdapter::with_service(Arc::new(WrongWinner(winner)));
    let result = adapter.dispatch_as(&other, command).await;
    eprintln!("cross-scope local result: {result:?}");
    assert!(
        result.is_err(),
        "typed adapter exposed scope-a completion to scope-b"
    );
    Ok(())
}

struct FailedPolicy(AuthorizationError);
#[async_trait]
impl CommandAuthorizer for FailedPolicy {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        Err(self.0)
    }
}
async fn provider_audit(error: AuthorizationError) -> TestResult {
    let audit = Arc::new(InMemoryCommandAuditSink::new(2));
    let service = CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
        .authorizer(Arc::new(FailedPolicy(error)))
        .audit_sink(audit.clone())
        .build()?;
    let report = CommandProcessingService::process(
        &service,
        &caller("scope")?,
        &encoded("provider-failure", false)?,
    )
    .await;
    assert_eq!(
        report.result().as_ref().unwrap_err().authorization_error(),
        Some(&error)
    );
    assert_eq!(report.audit(), &AuditDelivery::Delivered);
    let records = audit.records().await;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].attempt_id(), report.attempt_id());
    assert_eq!(records[1].attempt_id(), report.attempt_id());
    eprintln!("provider {error:?}, terminal {:?}", records[1].terminal());
    assert_eq!(
        records[1].terminal(),
        Some(AuditTerminal::Failed),
        "policy infrastructure failure is not a policy denial"
    );
    Ok(())
}
#[tokio::test]
async fn unavailable_provider_is_a_failed_audit_attempt_not_a_denial() -> TestResult {
    provider_audit(AuthorizationError::Unavailable).await
}
#[tokio::test]
async fn misconfigured_provider_is_a_failed_audit_attempt_not_a_denial() -> TestResult {
    provider_audit(AuthorizationError::InvalidConfiguration).await
}

#[tokio::test]
async fn executor_cannot_fork_shared_effective_codecs_after_selection() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    seed_v1(store.as_ref()).await?;
    let mut codecs = AggregateCodecs::default();
    codecs.register::<Counter, _>(CentsCodec {
        scale: 100,
        decodes: Arc::new(AtomicUsize::new(0)),
    })?;
    let codecs = Arc::new(codecs);
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("shared-codec")?,
        ContentFingerprint::digest("shared"),
    )
    .with_bounded_context(context()?.name().clone());
    let original = CommandExecutor::with_codecs(store.clone(), codecs.clone())
        .simulate(
            &ChangeHandler(Arc::new(AtomicUsize::new(0))),
            metadata.clone(),
            &Change {
                amount: 0,
                reject: true,
            },
        )
        .await?;
    assert_eq!(
        original.decision(),
        &CommandDecision::Rejected("balance=200".to_owned())
    );
    let fork = CommandExecutor::with_codecs(store.clone(), codecs.clone())
        .with_codec::<Counter, _>(CentsCodec {
            scale: 1,
            decodes: Arc::new(AtomicUsize::new(0)),
        });
    match fork {
        Err(CodecRegistrationError::SharedConfiguration) => Ok(()),
        Err(error) => Err(error.into()),
        Ok(fork) => {
            let changed = fork
                .simulate(
                    &ChangeHandler(Arc::new(AtomicUsize::new(0))),
                    metadata,
                    &Change {
                        amount: 0,
                        reject: true,
                    },
                )
                .await?;
            eprintln!(
                "same selected registry: original {:?}; overridden {:?}",
                original.decision(),
                changed.decision()
            );
            assert_eq!(
                changed.decision(),
                original.decision(),
                "shared registry silently forked after selection"
            );
            Err("shared configuration must reject local override even when meanings happen to agree".into())
        }
    }
}

struct SpyBackend {
    inner: InMemoryEventStore,
    reads: Arc<AtomicUsize>,
    starts: Arc<AtomicUsize>,
}
#[async_trait]
impl EventHistory for SpyBackend {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for SpyBackend {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl CommandBackend for SpyBackend {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        self.inner.begin_command().await
    }
}
struct CountPolicy(Arc<AtomicUsize>);
#[async_trait]
impl CommandAuthorizer for CountPolicy {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn inconsistent_original_envelope_is_refused_before_policy_audit_or_authority() -> TestResult
{
    let reads = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let policies = Arc::new(AtomicUsize::new(0));
    let audit = Arc::new(InMemoryCommandAuditSink::new(10));
    let service = CommandProcessor::builder(
        context()?,
        Arc::new(SpyBackend {
            inner: InMemoryEventStore::new(),
            reads: reads.clone(),
            starts: starts.clone(),
        }),
    )
    .authorizer(Arc::new(CountPolicy(policies.clone())))
    .audit_sink(audit.clone())
    .build()?;
    let command = encoded("inner-outer-identity", false)?;
    let inconsistent = EncodedCommand::from_delivery(
        command.address().clone(),
        rostfrei::MessageId::new("incorrect-outer-message")?,
        command.payload().to_vec(),
    )?;
    for report in [
        CommandProcessingService::process(&service, &caller("scope")?, &inconsistent)
            .await
            .into_result()
            .map(|_| ()),
        service
            .lookup(&caller("scope")?, &inconsistent)
            .await
            .into_result()
            .map(|_| ()),
    ] {
        assert_eq!(
            report.unwrap_err().kind(),
            rostfrei::CommandProcessorErrorKind::InvalidMessage
        );
    }
    assert_eq!(policies.load(Ordering::SeqCst), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert!(audit.records().await.is_empty());
    Ok(())
}

struct CorruptDetails(Arc<AtomicUsize>);
#[async_trait]
impl OutcomeDetailReader for CorruptDetails {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(DetailRead::Found(b"not the retained bytes".to_vec()))
    }
}
#[tokio::test]
async fn denied_completed_read_does_not_reach_corrupt_details_and_allowed_read_stays_blocked()
-> TestResult {
    let store = InMemoryEventStore::new();
    let command = encoded("corrupt-recovery", true)?;
    let who = caller("scope")?;
    CommandProcessingService::process(&processor(Arc::new(store.clone()))?, &who, &command)
        .await
        .into_result()?;
    let details = Arc::new(AtomicUsize::new(0));
    let reads = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(SpyBackend {
        inner: store.with_detail_readers(Arc::new(CorruptDetails(details.clone())), None),
        reads: reads.clone(),
        starts: starts.clone(),
    });
    let denied = CommandProcessor::builder(context()?, backend.clone())
        .authorizer(Arc::new(FailedPolicy(AuthorizationError::Denied)))
        .build()?;
    assert!(
        CommandProcessingService::process(&denied, &who, &command)
            .await
            .result()
            .is_err()
    );
    assert_eq!(details.load(Ordering::SeqCst), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let allowed = CommandProcessor::builder(context()?, backend)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    for result in [
        CommandProcessingService::process(&allowed, &who, &command)
            .await
            .into_result()
            .map(|_| ()),
        allowed
            .lookup(&who, &command)
            .await
            .into_result()
            .map(|_| ()),
    ] {
        assert_eq!(
            result.unwrap_err().kind(),
            rostfrei::CommandProcessorErrorKind::CompletedDetailsBlocked
        );
    }
    assert_eq!(details.load(Ordering::SeqCst), 2);
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    Ok(())
}

struct OrderedAudit {
    calls: Arc<AtomicUsize>,
    phases: Mutex<Vec<AuditPhase>>,
}
#[async_trait]
impl CommandAuditSink for OrderedAudit {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), rostfrei::AuditError> {
        assert_eq!(
            self.calls.load(Ordering::SeqCst),
            usize::from(record.phase() == AuditPhase::Finished)
        );
        tokio::task::yield_now().await;
        self.phases.lock().unwrap().push(record.phase());
        if record.phase() == AuditPhase::Finished {
            Err(rostfrei::AuditError::new("finish failed after commit"))
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn awaited_audit_failure_is_phase_specific_and_does_not_retry_handler() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let audit = Arc::new(OrderedAudit {
        calls: calls.clone(),
        phases: Mutex::new(Vec::new()),
    });
    let store = Arc::new(InMemoryEventStore::new());
    let mut service = CommandProcessor::builder(context()?, store.clone())
        .authorizer(Arc::new(AllowAllCommands))
        .audit_sink(audit.clone())
        .build()?;
    service.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let report = CommandProcessingService::process(
        &service,
        &caller("scope")?,
        &encoded("audit-phase-failure", false)?,
    )
    .await;
    assert_eq!(
        report.result().as_ref().unwrap().origin(),
        CompletionOrigin::Committed
    );
    let AuditDelivery::Failed { records } = report.audit() else {
        return Err("failure was hidden".into());
    };
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, AuditPhase::Finished);
    assert_eq!(
        *audit.phases.lock().unwrap(),
        [AuditPhase::Started, AuditPhase::Finished]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.load(&stream()?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn unscoped_legacy_completion_is_blocked_by_policy_enabled_service() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let command = encoded("legacy-no-authenticated-scope", false)?;
    baseline_processor(store.clone(), Arc::new(AtomicUsize::new(0)))?
        .process(&command)
        .await?;
    let reads = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let service = processor(Arc::new(SpyBackend {
        inner: store.as_ref().clone(),
        reads,
        starts: starts.clone(),
    }))?;
    let report = CommandProcessingService::process(&service, &caller("scope")?, &command).await;
    assert_eq!(
        report.result().as_ref().unwrap_err().kind(),
        rostfrei::CommandProcessorErrorKind::IdentityConflict
    );
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    Ok(())
}

/// Installs a real winner between the service's Missing read and executor lookup.
struct ConcurrentWinner {
    writer: CommandProcessor,
    store: Arc<InMemoryEventStore>,
    expire: bool,
    accesses: Mutex<Vec<CommandAccess>>,
}
#[async_trait]
impl CommandAuthorizer for ConcurrentWinner {
    async fn authorize(
        &self,
        who: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.accesses.lock().unwrap().push(access);
        if access == CommandAccess::Execute {
            let winner = CommandProcessingService::process(&self.writer, who, command.original())
                .await
                .into_result()
                .unwrap();
            if self.expire {
                self.store
                    .expire_details(
                        winner.outcome().marker().detail_reference().key(),
                        winner.outcome().marker().revision(),
                    )
                    .await
                    .unwrap();
            }
        }
        Ok(())
    }
}
#[tokio::test]
async fn concurrent_winner_expired_before_executor_lookup_never_evaluates_or_needs_binding()
-> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let audit = Arc::new(InMemoryCommandAuditSink::new(2));
    let policy = Arc::new(ConcurrentWinner {
        writer: processor(store.clone())?,
        store: store.clone(),
        expire: true,
        accesses: Mutex::new(Vec::new()),
    });
    // No binding on this processor. A mistaken Missing would write an unknown-command refusal.
    let outer = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy.clone())
        .audit_sink(audit.clone())
        .conflict_retry_policy(Arc::new(BoundedConflictRetries::new(0)))
        .build()?;
    let command = encoded("race-expired-winner", false)?;
    let report = CommandProcessingService::process(&outer, &caller("scope")?, &command).await;
    let winner = report.result().as_ref().unwrap();
    assert_eq!(winner.origin(), CompletionOrigin::Replayed);
    assert_eq!(
        winner.outcome().marker().decision(),
        OutcomeDecision::Accepted
    );
    assert_eq!(winner.outcome().details(), &OutcomeDetails::Expired);
    assert_eq!(
        *policy.accesses.lock().unwrap(),
        [CommandAccess::ReadOutcome, CommandAccess::Execute]
    );
    let records = audit.records().await;
    assert_eq!(records[1].terminal(), Some(AuditTerminal::Expired));
    assert_eq!(records[1].attempt_id(), report.attempt_id());
    assert_eq!(store.load(&stream()?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn concurrent_corrupt_winner_has_same_recovery_classification_as_initial_lookup() -> TestResult
{
    let store = Arc::new(InMemoryEventStore::new());
    let policy = Arc::new(ConcurrentWinner {
        writer: processor(store.clone())?,
        store: store.clone(),
        expire: false,
        accesses: Mutex::new(Vec::new()),
    });
    let faulty_reader = Arc::new(store.as_ref().clone().with_detail_readers(
        Arc::new(CorruptDetails(Arc::new(AtomicUsize::new(0)))),
        None,
    ));
    let outer = CommandProcessor::builder(context()?, faulty_reader)
        .authorizer(policy.clone())
        .build()?;
    let command = encoded("race-corrupt-winner", false)?;
    let who = caller("scope")?;
    let raced = CommandProcessingService::process(&outer, &who, &command)
        .await
        .into_result()
        .unwrap_err();
    let initial = CommandProcessingService::process(&outer, &who, &command)
        .await
        .into_result()
        .unwrap_err();
    assert_eq!(store.load(&stream()?).await?.len(), 1);
    assert_eq!(
        initial.kind(),
        rostfrei::CommandProcessorErrorKind::CompletedDetailsBlocked
    );
    eprintln!(
        "identical corrupt winner: initial={:?}, concurrent={:?}",
        initial.kind(),
        raced.kind()
    );
    assert_eq!(
        raced.kind(),
        initial.kind(),
        "winner recovery cannot change transport classification because it raced Missing"
    );
    Ok(())
}

#[tokio::test]
async fn transient_boundary_errors_remain_retryable_in_processor_and_bus() -> TestResult {
    let errors = [
        rostfrei::CommandProcessorError::authorization(AuthorizationError::Unavailable),
        rostfrei::CommandProcessorError::recovery(&OutcomeRecoveryError::Unavailable {
            reason: DetailUnavailableReason::BackendUnavailable,
        }),
        rostfrei::CommandProcessorError::recovery(&OutcomeRecoveryError::Archived {
            reference: ArchiveReference::new("pending-archive")?,
        }),
    ];
    let observations: Vec<_> = errors
        .iter()
        .map(|error| {
            (
                error.kind(),
                error.is_retryable(),
                rostfrei::CommandBusError::from_processor(error).is_retryable(),
            )
        })
        .collect();
    eprintln!("transient retry classifications (kind, processor, bus): {observations:?}");
    assert!(
        observations
            .iter()
            .all(|(_, processor, bus)| *processor && *bus),
        "new transient boundary states are excluded from public retry helpers"
    );
    Ok(())
}
