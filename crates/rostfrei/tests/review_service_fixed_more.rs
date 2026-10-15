//! Fresh independent repair probes, beyond the preserved original review tests.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{
    AllowAllCommands, AuditDelivery, AuditPhase, AuditTerminal, AuthorizationError, CommandAccess,
    CommandAuditRecord, CommandAuditSink, CommandAuthorizer, CommandProcessingService,
    CommandProcessorErrorKind, CompletionOrigin, PrincipalId, ProcessedCommand,
    TrustedCommandContext, ValidatedCommand,
};
rostfrei::install_macro_support!();

fn who() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("review-fixed")?,
        SecurityScope::new("scope")?,
    ))
}
fn encode(route: BoundedContext, request: CommandRequest<Change>) -> TestResult<EncodedCommand> {
    let encoder = CommandBus::new(
        route,
        Arc::new(InMemoryMessagingAdapter::new(baseline_processor(
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?)),
    );
    noncanonical(&encoder.encode(request)?)
}
fn processor(
    route: BoundedContext,
    store: Arc<dyn CommandBackend>,
    calls: Arc<AtomicUsize>,
) -> TestResult<CommandProcessor> {
    let mut service = CommandProcessor::builder(route, store)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    service.register::<Change, _>(ChangeHandler(calls), map_rejection)?;
    Ok(service)
}
async fn finish(route: BoundedContext, command: &EncodedCommand) -> TestResult<ProcessedCommand> {
    Ok(CommandProcessingService::process(
        &processor(
            route,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
        )?,
        &who()?,
        command,
    )
    .await
    .into_result()?)
}

#[tokio::test]
async fn same_operation_readdressed_response_cannot_hide_changed_payload_route_or_provenance()
-> TestResult {
    let original = encode(context()?, request("same-operation", true)?)?;
    let winner = finish(context()?, &original).await?;
    let cases = [
        (
            context()?,
            request("same-operation", true)?
                .with_correlation_id(rostfrei::CorrelationId::new("different-flow")?),
        ),
        (
            context()?,
            request("same-operation", true)?
                .with_causation_id(rostfrei::CausationId::new("different-cause")?),
        ),
        (
            ApplicationName::new("different-app")?.bounded_context("modular")?,
            request("same-operation", true)?,
        ),
        (
            context()?.application().test_bounded_context("modular")?,
            request("same-operation", true)?,
        ),
        (
            context()?,
            CommandRequest::new(
                OperationId::new("same-operation")?,
                Change {
                    amount: 99,
                    reject: true,
                },
            ),
        ),
    ];
    for (route, request) in cases {
        let command = encode(route.clone(), request)?;
        let alternate = finish(route, &command).await?;
        let response = alternate.outcome().details().available()?.clone();
        command.validate_response(&response)?;
        assert_eq!(original.operation_id(), command.operation_id());
        assert_eq!(
            winner.outcome().marker().decision(),
            alternate.outcome().marker().decision()
        );
        let mapped = ProcessedCommand::new(
            CompletionOrigin::Replayed,
            winner
                .outcome()
                .clone()
                .try_map::<_, OutcomeRecoveryError>(|_| Ok(response))?,
        );
        assert_eq!(
            mapped.admit(&who()?, &command).unwrap_err().kind(),
            CommandProcessorErrorKind::CompletedDetailsBlocked
        );
    }
    // Timestamp is not command identity, and another authorized principal is not a namespace.
    let equivalent = encode(
        context()?,
        request("same-operation", true)?.with_created_at(
            rostfrei::MessageTimestamp::from_unix_milliseconds(1_800_000_000_000)?,
        ),
    )?;
    let other = TrustedCommandContext::new(
        PrincipalId::new("other-authorized-user")?,
        SecurityScope::new("scope")?,
    );
    assert_eq!(
        winner.admit(&other, &equivalent)?,
        winner.outcome().details().available()?
    );
    Ok(())
}

#[tokio::test]
async fn identity_checks_precede_unavailable_state_projection_in_checked_admission() -> TestResult {
    let command = encode(context()?, request("unavailable-admission", true)?)?;
    let winner = finish(context()?, &command).await?;
    let other = TrustedCommandContext::new(
        PrincipalId::new("review-fixed")?,
        SecurityScope::new("other-scope")?,
    );
    for (details, kind) in [
        (
            OutcomeDetails::Expired,
            CommandProcessorErrorKind::CompletedDetailsExpired,
        ),
        (
            OutcomeDetails::Archived {
                reference: ArchiveReference::new("archive")?,
            },
            CommandProcessorErrorKind::CompletedDetailsUnavailable,
        ),
        (
            OutcomeDetails::Unavailable {
                reason: DetailUnavailableReason::MissingDetails,
            },
            CommandProcessorErrorKind::CompletedDetailsBlocked,
        ),
        (
            OutcomeDetails::Unavailable {
                reason: DetailUnavailableReason::BackendUnavailable,
            },
            CommandProcessorErrorKind::CompletedDetailsUnavailable,
        ),
    ] {
        let result = ProcessedCommand::new(
            CompletionOrigin::Replayed,
            CompletedOutcome::new(winner.outcome().marker().clone(), details)?,
        );
        assert_eq!(result.admit(&who()?, &command).unwrap_err().kind(), kind);
        assert_eq!(
            result.admit(&other, &command).unwrap_err().kind(),
            CommandProcessorErrorKind::CompletedDetailsBlocked
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Window {
    History,
    BeforeWrite,
    LostAck,
    AfterDurable,
}
struct FaultBackend {
    inner: InMemoryEventStore,
    window: Window,
    kind: EventStoreErrorKind,
}
impl FaultBackend {
    fn fault(&self) -> EventStoreError {
        EventStoreError::new(self.kind, "same controlled failure text")
    }
}
#[async_trait]
impl EventHistory for FaultBackend {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for FaultBackend {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl CommandBackend for FaultBackend {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(FaultSession {
            backend: self,
            inner: self.inner.begin_command().await?,
        }))
    }
}
struct FaultSession<'a> {
    backend: &'a FaultBackend,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for FaultSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        if matches!(self.backend.window, Window::History) {
            Err(self.backend.fault())
        } else {
            self.inner.load(id).await
        }
    }
}
#[async_trait]
impl CommandCommitSession for FaultSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        if matches!(self.backend.window, Window::BeforeWrite) {
            return Err(self.backend.fault());
        }
        self.inner.commit_outcome(proposal).await?;
        Err(if matches!(self.backend.window, Window::AfterDurable) {
            self.backend.fault().in_outcome_recovery()
        } else {
            self.backend.fault()
        })
    }
}

#[tokio::test]
async fn outcome_origin_does_not_erase_history_or_uncertain_rejection_persistence_semantics()
-> TestResult {
    for reject in [false, true] {
        for kind in [
            EventStoreErrorKind::CorruptHistory,
            EventStoreErrorKind::ConfigurationMismatch,
            EventStoreErrorKind::Unavailable,
        ] {
            for window in [
                Window::History,
                Window::BeforeWrite,
                Window::LostAck,
                Window::AfterDurable,
            ] {
                let store = InMemoryEventStore::new();
                let calls = Arc::new(AtomicUsize::new(0));
                let service = processor(
                    context()?,
                    Arc::new(FaultBackend {
                        inner: store.clone(),
                        window,
                        kind,
                    }),
                    calls.clone(),
                )?;
                let command = encode(context()?, request("origin-probe", reject)?)?;
                let error = CommandProcessingService::process(&service, &who()?, &command)
                    .await
                    .into_result()
                    .unwrap_err();
                assert_eq!(
                    error.kind(),
                    if kind == EventStoreErrorKind::Unavailable {
                        CommandProcessorErrorKind::Unavailable
                    } else if matches!(window, Window::AfterDurable) {
                        CommandProcessorErrorKind::CompletedDetailsBlocked
                    } else {
                        CommandProcessorErrorKind::ExecutionFailed
                    },
                    "{window:?}, {kind:?}, reject={reject}"
                );
                assert_eq!(
                    error.is_retryable(),
                    kind == EventStoreErrorKind::Unavailable
                );
                assert_eq!(
                    rostfrei::CommandBusError::from_processor(&error).is_retryable(),
                    kind == EventStoreErrorKind::Unavailable
                );
                let diagnostic = error.technical_response(&command)?;
                let suppressed = (matches!(window, Window::AfterDurable)
                    && kind != EventStoreErrorKind::Unavailable)
                    || (reject && matches!(window, Window::BeforeWrite | Window::LostAck));
                assert_eq!(
                    diagnostic.is_none(),
                    suppressed,
                    "{window:?}, reject={reject}"
                );
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                // Recovery must not need any binding, even after an uncertain acknowledgement.
                let unbound = CommandProcessor::builder(context()?, Arc::new(store.clone()))
                    .authorizer(Arc::new(AllowAllCommands))
                    .build()?;
                let lookup = unbound.lookup(&who()?, &command).await.into_result()?;
                if matches!(window, Window::AfterDurable | Window::LostAck) {
                    assert!(matches!(lookup, OutcomeLookup::Completed(_)));
                    let replay = CommandProcessingService::process(&unbound, &who()?, &command)
                        .await
                        .into_result()?;
                    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
                    assert_eq!(
                        replay.outcome().marker().decision(),
                        if reject {
                            OutcomeDecision::Rejected
                        } else {
                            OutcomeDecision::Accepted
                        }
                    );
                    replay.admit(&who()?, &command)?;
                } else {
                    assert!(matches!(lookup, OutcomeLookup::Missing));
                }
                assert_eq!(
                    store.load(&stream()?).await?.len(),
                    usize::from(
                        !reject && matches!(window, Window::AfterDurable | Window::LostAck)
                    )
                );
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
        }
    }
    Ok(())
}

struct MatrixPolicy {
    access: CommandAccess,
    error: AuthorizationError,
    deny: AtomicBool,
    seen: Mutex<Vec<CommandAccess>>,
}
#[async_trait]
impl CommandAuthorizer for MatrixPolicy {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.seen.lock().unwrap().push(access);
        if access == self.access && self.deny.load(Ordering::SeqCst) {
            Err(self.error)
        } else {
            Ok(())
        }
    }
}
struct FinishFailure(Mutex<Vec<CommandAuditRecord>>);
#[async_trait]
impl CommandAuditSink for FinishFailure {
    async fn record(&self, record: &CommandAuditRecord) -> Result<(), rostfrei::AuditError> {
        self.0.lock().unwrap().push(record.clone());
        if record.phase() == AuditPhase::Finished {
            Err(rostfrei::AuditError::new("independent finish outage"))
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn read_and_execute_denial_matrix_keeps_policy_audit_delivery_and_retry_independent()
-> TestResult {
    for access in [CommandAccess::ReadOutcome, CommandAccess::Execute] {
        for error in [
            AuthorizationError::Denied,
            AuthorizationError::Unauthenticated,
            AuthorizationError::Unavailable,
            AuthorizationError::InvalidConfiguration,
        ] {
            let store = Arc::new(InMemoryEventStore::new());
            let policy = Arc::new(MatrixPolicy {
                access,
                error,
                deny: AtomicBool::new(true),
                seen: Mutex::new(Vec::new()),
            });
            let audit = Arc::new(FinishFailure(Mutex::new(Vec::new())));
            let calls = Arc::new(AtomicUsize::new(0));
            let mut service = CommandProcessor::builder(context()?, store.clone())
                .authorizer(policy.clone())
                .audit_sink(audit.clone())
                .build()?;
            service.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
            let command = encode(context()?, request("audit-matrix", false)?)?;
            let caller = who()?;
            let report = CommandProcessingService::process(&service, &caller, &command).await;
            let boundary = report.result().as_ref().unwrap_err();
            assert_eq!(boundary.authorization_error(), Some(&error));
            assert_eq!(
                boundary.is_retryable(),
                error == AuthorizationError::Unavailable
            );
            assert_eq!(
                rostfrei::CommandBusError::from_processor(boundary).is_retryable(),
                error == AuthorizationError::Unavailable
            );
            let AuditDelivery::Failed { records } = report.audit() else {
                return Err("audit failure not reported".into());
            };
            let [(phase, _)] = records.as_slice() else {
                return Err("wrong failed phases".into());
            };
            assert_eq!(*phase, AuditPhase::Finished);
            let records = audit.0.lock().unwrap().clone();
            let [started, finished] = records.as_slice() else {
                return Err("incorrect audit record count".into());
            };
            assert_eq!(started.phase(), AuditPhase::Started);
            assert_eq!(started.terminal(), None);
            for record in &records {
                assert_eq!(record.attempt_id(), report.attempt_id());
                assert_eq!(record.original_payload(), command.payload());
                assert_eq!(record.security_scope(), caller.security_scope());
                assert_eq!(record.principal(), caller.principal());
            }
            assert_eq!(
                finished.terminal(),
                Some(
                    if matches!(
                        error,
                        AuthorizationError::Denied | AuthorizationError::Unauthenticated
                    ) {
                        AuditTerminal::Denied
                    } else {
                        AuditTerminal::Failed
                    }
                )
            );
            let seen = policy.seen.lock().unwrap().clone();
            assert_eq!(
                seen,
                if access == CommandAccess::ReadOutcome {
                    vec![CommandAccess::ReadOutcome]
                } else {
                    vec![CommandAccess::ReadOutcome, CommandAccess::Execute]
                }
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let key = OutcomeKey::new(
                store.authority().clone(),
                context()?.name().clone(),
                OperationId::new(command.operation_id().as_str())?,
            );
            assert!(matches!(
                store.lookup_outcome(&key).await?,
                OutcomeLookup::Missing
            ));
            policy.deny.store(false, Ordering::SeqCst);
            let allowed = CommandProcessingService::process(&service, &caller, &command).await;
            assert_eq!(
                allowed.result().as_ref().unwrap().origin(),
                CompletionOrigin::Committed
            );
            assert_ne!(allowed.attempt_id(), report.attempt_id());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    Ok(())
}
