#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod modular_support;
mod service_fix_support;
use async_trait::async_trait;
use modular_support::*;
use service_fix_support::*;
rostfrei::install_macro_support!();

#[tokio::test]
async fn mapped_decision_and_response_provenance_are_checked_before_local_publication() -> TestResult
{
    for reject in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = processor(Arc::new(InMemoryEventStore::new()), calls.clone())?;
        let command = encoded("mapped-result", reject)?;
        let who = caller()?;
        let winner = CommandProcessingService::process(&service, &who, &command)
            .await
            .into_result()?;
        let response = opposite_response(winner.outcome().details().available()?)?;
        command.validate_response(&response)?; // Valid request identity, wrong permanent decision.
        let substituted = substitute_response(&winner, response)?;
        assert_eq!(
            substituted.admit(&who, &command).unwrap_err().kind(),
            CommandProcessorErrorKind::CompletedDetailsBlocked
        );
        let adapter = InMemoryMessagingAdapter::with_service(Arc::new(Substitute(substituted)));
        let error = adapter
            .dispatch_as(&who, command.clone())
            .await
            .unwrap_err();
        assert_eq!(
            error.kind(),
            rostfrei::CommandBusErrorKind::CompletedDetailsBlocked
        );
        assert!(!error.is_retryable());
        assert!(adapter.command_messages().await.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let original = winner.outcome().details().available()?;
        for (field, replacement) in [
            ("message_id", "wrong-response"),
            ("command_message_id", "wrong-command"),
            ("command_address", "other.command.modular.change"),
            ("operation_id", "wrong-operation"),
            ("correlation_id", "wrong-correlation"),
        ] {
            let mut json = serde_json::to_value(original)?;
            *json.get_mut(field).ok_or("missing response field")? = replacement.into();
            let response: CommandResponse = serde_json::from_value(json)?;
            let adapter = InMemoryMessagingAdapter::with_service(Arc::new(Substitute(
                substitute_response(&winner, response)?,
            )));
            assert_eq!(
                adapter
                    .dispatch_as(&who, command.clone())
                    .await
                    .unwrap_err()
                    .kind(),
                rostfrei::CommandBusErrorKind::CompletedDetailsBlocked,
                "{field}"
            );
            assert!(adapter.command_messages().await.is_empty());
        }
        let technical = CommandResponse::execution_failed(
            rostfrei::command_technical_response_message_id(
                command.message_id(),
                rostfrei::ExecutionFailure::Store(rostfrei::StoreFailureKind::Unavailable),
            )?,
            command.message_id().clone(),
            command.address().clone(),
            command.operation_id().clone(),
            command.correlation_id().clone(),
            rostfrei::ExecutionFailure::Store(rostfrei::StoreFailureKind::Unavailable),
        )?;
        command.validate_response(&technical)?;
        assert_eq!(
            substitute_response(&winner, technical)?
                .admit(&who, &command)
                .unwrap_err()
                .kind(),
            CommandProcessorErrorKind::CompletedDetailsBlocked
        );
        assert_eq!(winner.admit(&who, &command)?, original);
    }
    Ok(())
}

#[tokio::test]
async fn commit_recovery_and_conflict_recovery_keep_blocked_origin_for_both_decisions() -> TestResult
{
    for fault in [
        DetailReadError::Corrupt,
        DetailReadError::Forbidden,
        DetailReadError::ConfigurationMismatch,
    ] {
        for reject in [false, true] {
            for race in [false, true] {
                let store = InMemoryEventStore::new();
                let faulty = store
                    .clone()
                    .with_detail_readers(Arc::new(FaultReader(fault.clone())), None);
                let calls = Arc::new(AtomicUsize::new(0));
                let winner_calls = Arc::new(AtomicUsize::new(0));
                let command = encoded("commit-recovery", reject)?;
                let who = caller()?;
                let mut writer = CommandProcessor::builder(context()?, Arc::new(store.clone()))
                    .authorizer(Arc::new(AllowAllCommands))
                    .build()?;
                writer
                    .register::<Change, _>(EventFreeWinner(winner_calls.clone()), map_rejection)?;
                let racing = Arc::new(CommitRace {
                    inner: faulty.clone(),
                    writer,
                    command: command.clone(),
                    who: who.clone(),
                    starts: AtomicUsize::new(0),
                    conflicts: AtomicUsize::new(0),
                });
                let backend: Arc<dyn CommandBackend> = if race {
                    racing.clone()
                } else {
                    Arc::new(faulty)
                };
                let mut service = CommandProcessor::builder(context()?, backend)
                    .authorizer(Arc::new(AllowAllCommands))
                    .conflict_retry_policy(Arc::new(NoConflictRetries))
                    .build()?;
                service.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
                for _ in 0..2 {
                    // First commit/reconciliation, then initial completed lookup.
                    let error = CommandProcessingService::process(&service, &who, &command)
                        .await
                        .into_result()
                        .unwrap_err();
                    assert_eq!(
                        error.kind(),
                        CommandProcessorErrorKind::CompletedDetailsBlocked
                    );
                    assert!(!error.is_retryable());
                    assert!(error.technical_response(&command)?.is_none());
                }
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                assert_eq!(winner_calls.load(Ordering::SeqCst), usize::from(race));
                assert_eq!(racing.starts.load(Ordering::SeqCst), usize::from(race));
                assert_eq!(racing.conflicts.load(Ordering::SeqCst), usize::from(race));
                assert_eq!(
                    store.load(&stream()?).await?.len(),
                    usize::from(!race && !reject)
                );
                let readable = processor(Arc::new(store), Arc::new(AtomicUsize::new(0)))?;
                let recovered = CommandProcessingService::process(&readable, &who, &command)
                    .await
                    .into_result()?;
                assert_eq!(recovered.origin(), CompletionOrigin::Replayed);
                assert_eq!(
                    recovered.outcome().marker().decision(),
                    if reject && !race {
                        OutcomeDecision::Rejected
                    } else {
                        OutcomeDecision::Accepted
                    }
                );
            }
        }
    }
    Ok(())
}

struct RecoverableReader {
    inner: Arc<dyn OutcomeDetailReader>,
    online: AtomicBool,
}
#[async_trait]
impl OutcomeDetailReader for RecoverableReader {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        if self.online.load(Ordering::SeqCst) {
            self.inner.get(reference).await
        } else {
            Err(DetailReadError::Unavailable)
        }
    }
}
struct Accesses(Mutex<Vec<CommandAccess>>);
#[async_trait]
impl CommandAuthorizer for Accesses {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.0.lock().unwrap().push(access);
        Ok(())
    }
}
#[tokio::test]
async fn retry_completed_detail_outage_reauthorizes_same_identity_without_reexecution() -> TestResult
{
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let command = encoded("recover-same-operation", true)?;
    let who = caller()?;
    let winner = CommandProcessingService::process(
        &processor(store.clone(), calls.clone())?,
        &who,
        &command,
    )
    .await
    .into_result()?;
    let reader = Arc::new(RecoverableReader {
        inner: store.detail_reader(),
        online: AtomicBool::new(false),
    });
    let policy = Arc::new(Accesses(Mutex::new(vec![])));
    let mut service = CommandProcessor::builder(
        context()?,
        Arc::new(
            store
                .as_ref()
                .clone()
                .with_detail_readers(reader.clone(), None),
        ),
    )
    .authorizer(policy.clone())
    .build()?;
    service.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let adapter = InMemoryMessagingAdapter::with_service(Arc::new(service));
    let unavailable = adapter
        .dispatch_as(&who, command.clone())
        .await
        .unwrap_err();
    assert_eq!(
        unavailable.kind(),
        rostfrei::CommandBusErrorKind::CompletedDetailsUnavailable
    );
    assert!(unavailable.is_retryable());
    assert!(adapter.command_messages().await.is_empty());
    reader.online.store(true, Ordering::SeqCst);
    let recovered = adapter.dispatch_as(&who, command.clone()).await?;
    assert_eq!(
        recovered.response(),
        winner.outcome().details().available()?
    );
    assert_eq!(
        *policy.0.lock().unwrap(),
        [CommandAccess::ReadOutcome, CommandAccess::ReadOutcome]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.command_messages().await, [command]);
    Ok(())
}

struct Denial(AuthorizationError);
#[async_trait]
impl CommandAuthorizer for Denial {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        Err(self.0)
    }
}
#[tokio::test]
async fn permanent_boundary_failures_stay_permanent_and_actual_denials_keep_denied_audit()
-> TestResult {
    use rostfrei::{AuditPhase, AuditTerminal, CommandProcessorError};
    for error in [
        AuthorizationError::Denied,
        AuthorizationError::Unauthenticated,
        AuthorizationError::InvalidConfiguration,
    ] {
        let typed = CommandProcessorError::authorization(error);
        assert!(!typed.is_retryable());
        assert!(!rostfrei::CommandBusError::from_processor(&typed).is_retryable());
        let audit = Arc::new(rostfrei::InMemoryCommandAuditSink::new(2));
        let service = CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
            .authorizer(Arc::new(Denial(error)))
            .audit_sink(audit.clone())
            .build()?;
        let report = CommandProcessingService::process(
            &service,
            &caller()?,
            &encoded("audit-denial", false)?,
        )
        .await;
        assert_eq!(report.audit(), &AuditDelivery::Delivered);
        let records = audit.records().await;
        let [started, finished] = records.as_slice() else {
            return Err("missing audit phases".into());
        };
        assert_eq!(started.phase(), AuditPhase::Started);
        assert_eq!(finished.phase(), AuditPhase::Finished);
        assert_eq!(started.attempt_id(), report.attempt_id());
        assert_eq!(finished.attempt_id(), report.attempt_id());
        assert_eq!(
            finished.terminal(),
            Some(if error == AuthorizationError::InvalidConfiguration {
                AuditTerminal::Failed
            } else {
                AuditTerminal::Denied
            })
        );
    }
    for recovery in [
        OutcomeRecoveryError::Expired,
        OutcomeRecoveryError::IntegrityMismatch,
        OutcomeRecoveryError::Unavailable {
            reason: DetailUnavailableReason::MissingDetails,
        },
    ] {
        let error = CommandProcessorError::recovery(&recovery);
        assert!(!error.is_retryable());
        assert!(!rostfrei::CommandBusError::from_processor(&error).is_retryable());
    }
    Ok(())
}
