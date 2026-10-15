//! Future-API acceptance. Baseline compile gaps are NOT behavioral REDs.
//! Exact interface choices and matrix mapping: docs/testing/modular-runtime-tdd.md.
#![allow(
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
use rostfrei::{
    AllowAllCommands, AuditDelivery, AuthorizationError, CommandAccess, CommandAuditRecord,
    CommandAuditSink, CommandAuthorizer, CommandProcessingService, CompletionOrigin,
    InMemoryCommandAuditSink, PrincipalId, SecurityScope, TrustedCommandContext, ValidatedCommand,
};
use std::time::Duration;
rostfrei::install_macro_support!();

fn caller(principal: &str, scope: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new(principal)?,
        SecurityScope::new(scope)?,
    ))
}
fn encoded(id: &str, reject: bool) -> TestResult<EncodedCommand> {
    let command = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(request(id, reject)?)?;
    noncanonical(&command)
}

type PolicyObservation = (CommandAccess, String, String, Vec<u8>);
#[derive(Default)]
struct Policy {
    deny_read: AtomicBool,
    deny_execute: AtomicBool,
    unavailable: AtomicBool,
    seen: Mutex<Vec<PolicyObservation>>,
}
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.seen.lock().unwrap().push((
            access,
            caller.principal().as_str().to_owned(),
            caller.security_scope().as_str().to_owned(),
            command.original().payload().to_vec(),
        ));
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(AuthorizationError::Unavailable);
        }
        if (access == CommandAccess::ReadOutcome && self.deny_read.load(Ordering::SeqCst))
            || (access == CommandAccess::Execute && self.deny_execute.load(Ordering::SeqCst))
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(())
    }
}
fn processor(
    store: Arc<InMemoryEventStore>,
    calls: Arc<AtomicUsize>,
    policy: Arc<dyn CommandAuthorizer>,
) -> TestResult<CommandProcessor> {
    let mut processor = CommandProcessor::builder(context()?, store)
        .authorizer(policy)
        .build()?;
    processor.register::<Change, _>(ChangeHandler(calls), map_rejection)?;
    Ok(processor)
}

#[tokio::test]
async fn missing_identity_needs_read_then_execute_and_denials_never_claim_it() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let processor = processor(store, calls.clone(), policy.clone())?;
    let caller = caller("verified-user", "tenant-a")?;
    let command = encoded("denial-is-not-completion", false)?;
    policy.deny_execute.store(true, Ordering::SeqCst);
    let denied = CommandProcessingService::process(&processor, &caller, &command).await;
    assert_eq!(
        denied.result().as_ref().unwrap_err().authorization_error(),
        Some(&AuthorizationError::Denied)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        processor.lookup(&caller, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    policy.deny_execute.store(false, Ordering::SeqCst);
    let committed = CommandProcessingService::process(&processor, &caller, &command)
        .await
        .into_result()?;
    assert_eq!(committed.origin(), CompletionOrigin::Committed);
    policy.deny_execute.store(true, Ordering::SeqCst);
    let replay = CommandProcessingService::process(&processor, &caller, &command)
        .await
        .into_result()?;
    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let accesses: Vec<_> = policy.seen.lock().unwrap().iter().map(|r| r.0).collect();
    assert_eq!(
        accesses,
        [
            CommandAccess::ReadOutcome,
            CommandAccess::Execute,
            CommandAccess::ReadOutcome,
            CommandAccess::ReadOutcome,
            CommandAccess::Execute,
            CommandAccess::ReadOutcome
        ]
    );
    Ok(())
}

#[tokio::test]
async fn local_cached_acceptance_and_rejection_are_reauthorized_on_every_call() -> TestResult {
    for reject in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let policy = Arc::new(Policy::default());
        let service: Arc<dyn CommandProcessingService> = Arc::new(processor(
            Arc::new(InMemoryEventStore::new()),
            calls.clone(),
            policy.clone(),
        )?);
        let adapter = InMemoryMessagingAdapter::with_service(service);
        let who = caller("alice", "tenant-a")?;
        let command = encoded(
            if reject {
                "cached-rejection"
            } else {
                "cached-acceptance"
            },
            reject,
        )?;
        let first = adapter.dispatch_as(&who, command.clone()).await?;
        policy.deny_read.store(true, Ordering::SeqCst);
        let error = adapter
            .dispatch_as(&who, command.clone())
            .await
            .unwrap_err();
        assert_eq!(
            error.authorization_error(),
            Some(&AuthorizationError::Denied)
        );
        policy.deny_read.store(false, Ordering::SeqCst);
        policy.deny_execute.store(true, Ordering::SeqCst);
        let other_authorized_principal = caller("bob", "tenant-a")?;
        let replay = adapter
            .dispatch_as(&other_authorized_principal, command)
            .await?;
        assert_eq!(first.response(), replay.response());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "principal is not an operation namespace"
        );
        assert_eq!(
            policy
                .seen
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.0 == CommandAccess::ReadOutcome)
                .count(),
            3
        );
    }
    Ok(())
}

#[tokio::test]
async fn trusted_scope_original_bytes_and_complete_route_survive_the_boundary() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let processor = processor(
        Arc::new(InMemoryEventStore::new()),
        calls.clone(),
        policy.clone(),
    )?;
    let who = caller("verified", "tenant-a")?;
    let command = encoded("scope-evidence", true)?;
    CommandProcessingService::process(&processor, &who, &command)
        .await
        .into_result()?;
    let changed = caller("verified", "tenant-b")?;
    assert!(processor.lookup(&changed, &command).await.result().is_err());
    assert!(
        CommandProcessingService::process(&processor, &changed, &command)
            .await
            .result()
            .is_err()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "scope cannot reopen an operation"
    );
    for row in policy.seen.lock().unwrap().iter() {
        assert_eq!(row.3, command.payload());
    }
    // Same bounded-context name is insufficient: application/Normal-vs-Test must match too.
    for wrong_context in [
        ApplicationName::new("another-app")?.bounded_context("modular")?,
        context()?.application().test_bounded_context("modular")?,
    ] {
        let bus = CommandBus::new(
            wrong_context,
            Arc::new(InMemoryMessagingAdapter::new(baseline_processor(
                Arc::new(InMemoryEventStore::new()),
                calls.clone(),
            )?)),
        );
        let wrong_route = bus.encode(request("wrong-route", false)?)?;
        let before = policy.seen.lock().unwrap().len();
        assert!(
            CommandProcessingService::process(&processor, &who, &wrong_route)
                .await
                .result()
                .is_err()
        );
        assert_eq!(
            policy.seen.lock().unwrap().len(),
            before,
            "route validation precedes authorizer"
        );
    }
    Ok(())
}

struct RaceOnce {
    store: Arc<InMemoryEventStore>,
    observations: Arc<Mutex<Vec<i64>>>,
}
#[async_trait]
impl CommandHandler<Change> for RaceOnce {
    type Rejection = String;
    async fn handle(
        &self,
        _: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let counter = execution.load::<Counter>("one").await?;
        let first = {
            let mut seen = self.observations.lock().unwrap();
            seen.push(*counter.state());
            seen.len() == 1
        };
        if first {
            let operation = OperationId::new("competitor").unwrap();
            let commit = derive_commit_id(&stream().unwrap(), &operation);
            let event = derive_event_id(&commit, 0);
            self.store
                .append(
                    &stream().unwrap(),
                    ExpectedVersion::NoStream,
                    EventBatch::new(
                        commit,
                        operation,
                        ContentFingerprint::digest("competitor"),
                        vec![NewEvent::new(event, "added", 2, b"10".to_vec()).unwrap()],
                    )
                    .unwrap(),
                )
                .await?;
        }
        Ok(CommandDecision::Rejected(format!(
            "observed={}",
            counter.state()
        )))
    }
}
#[tokio::test]
async fn injected_zero_and_one_conflict_budgets_keep_replay_and_fresh_read_guards() -> TestResult {
    for retries in [0, 1] {
        let store = Arc::new(InMemoryEventStore::new());
        let observations = Arc::new(Mutex::new(Vec::new()));
        let policy = Arc::new(Policy::default());
        let audit = Arc::new(InMemoryCommandAuditSink::new(8));
        let mut processor = CommandProcessor::builder(context()?, store.clone())
            .authorizer(policy.clone())
            .audit_sink(audit.clone())
            .conflict_retry_policy(Arc::new(BoundedConflictRetries::new(retries)))
            .build()?;
        processor.register::<Change, _>(
            RaceOnce {
                store,
                observations: observations.clone(),
            },
            map_rejection,
        )?;
        let who = caller("service", "tenant-a")?;
        let command = encoded("guarded-rejection", true)?;
        let result = CommandProcessingService::process(&processor, &who, &command)
            .await
            .into_result();
        let records = audit.records().await;
        assert_eq!(
            records.len(),
            2,
            "a conflict retry is part of one invocation, not a new attempt"
        );
        assert_eq!(records[0].attempt_id(), records[1].attempt_id());
        if retries == 0 {
            assert!(result.is_err());
            assert_eq!(*observations.lock().unwrap(), [0]);
            assert!(matches!(
                processor.lookup(&who, &command).await.into_result()?,
                OutcomeLookup::Missing
            ));
        } else {
            let committed = result?;
            assert_eq!(committed.origin(), CompletionOrigin::Committed);
            assert_eq!(*observations.lock().unwrap(), [0, 10]);
            assert_eq!(
                policy.seen.lock().unwrap().len(),
                2,
                "internal retry must not reauthorize"
            );
            assert_eq!(
                CommandProcessingService::process(&processor, &who, &command)
                    .await
                    .into_result()?
                    .origin(),
                CompletionOrigin::Replayed
            );
            assert_eq!(*observations.lock().unwrap(), [0, 10]);
        }
    }
    Ok(())
}

struct WireRejection {
    encodes: Arc<AtomicUsize>,
    decodes: Arc<AtomicUsize>,
    broken: bool,
}
impl RejectionCodec<CommandRejection> for WireRejection {
    fn encode(&self, value: &CommandRejection) -> Result<EncodedRejection, EventCodecError> {
        self.encodes.fetch_add(1, Ordering::SeqCst);
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.reverse();
        Ok(EncodedRejection {
            codec_id: "modular.reverse-json".to_owned(),
            codec_version: 9,
            bytes,
        })
    }
    fn decode(&self, value: &EncodedRejection) -> Result<CommandRejection, EventCodecError> {
        self.decodes.fetch_add(1, Ordering::SeqCst);
        if self.broken || value.codec_id != "modular.reverse-json" || value.codec_version != 9 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "controlled codec refusal",
            ));
        }
        let mut bytes = value.bytes.clone();
        bytes.reverse();
        serde_json::from_slice(&bytes)
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::MalformedPayload, e.to_string()))
    }
}
#[tokio::test]
async fn supplied_rejection_codec_covers_handler_framework_and_binding_free_replay() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let maps = Arc::new(AtomicUsize::new(0));
    let encodes = Arc::new(AtomicUsize::new(0));
    let decodes = Arc::new(AtomicUsize::new(0));
    let codec: Arc<dyn RejectionCodec<CommandRejection>> = Arc::new(WireRejection {
        encodes: encodes.clone(),
        decodes: decodes.clone(),
        broken: false,
    });
    let make = || {
        CommandProcessor::builder(context().unwrap(), store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .rejection_codec(codec.clone())
            .build()
    };
    let mut first = make()?;
    let map_calls = maps.clone();
    first.register::<Change, _>(ChangeHandler(calls.clone()), move |value: &String| {
        map_calls.fetch_add(1, Ordering::SeqCst);
        map_rejection(value)
    })?;
    let who = caller("service", "tenant-a")?;
    let command = encoded("custom-wire-rejection", true)?;
    let initial = CommandProcessingService::process(&first, &who, &command)
        .await
        .into_result()?;
    let before = encodes.load(Ordering::SeqCst);
    assert!(before > 0);
    let unbound = make()?;
    let replay = CommandProcessingService::process(&unbound, &who, &command)
        .await
        .into_result()?;
    assert_eq!(initial.outcome(), replay.outcome());
    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
    assert_eq!(
        encodes.load(Ordering::SeqCst),
        before,
        "replay must not re-encode"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(maps.load(Ordering::SeqCst), 1);
    // An unbound *new* operation is a durable framework refusal in the same configured format.
    let unknown = encoded("unbound-framework-refusal", false)?;
    let refusal = CommandProcessingService::process(&unbound, &who, &unknown)
        .await
        .into_result()?;
    assert!(
        matches!(refusal.outcome().details(), OutcomeDetails::Available(response) if matches!(response.outcome(), CommandResponseOutcome::Rejected(_)))
    );
    assert!(encodes.load(Ordering::SeqCst) > before);
    assert_eq!(
        CommandProcessingService::process(&make()?, &who, &unknown)
            .await
            .into_result()?
            .outcome(),
        refusal.outcome()
    );
    // The generic executor bridge must read the exact same mapped codec and trusted scope.
    let bridge = rostfrei::MappedCommandRejectionCodec::with_codec(
        map_rejection,
        |r: &CommandRejection| Ok(r.message().to_owned()),
        codec,
    );
    let generic = CommandExecutor::new(store)
        .execute_with_rejection_codec(
            &ChangeHandler(calls.clone()),
            command
                .validated_execution_metadata()?
                .with_security_scope(who.security_scope().clone()),
            &Change {
                amount: 7,
                reject: true,
            },
            &bridge,
        )
        .await?;
    assert_eq!(generic, CommandOutcome::Rejected("balance=0".to_owned()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn rejection_roundtrip_failure_never_commits_and_authorizer_outage_is_not_a_rejection()
-> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy.clone())
        .rejection_codec(Arc::new(WireRejection {
            encodes: Arc::new(AtomicUsize::new(0)),
            decodes: Arc::new(AtomicUsize::new(0)),
            broken: true,
        }))
        .build()?;
    processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let who = caller("service", "tenant-a")?;
    let command = encoded("codec-roundtrip-failure", true)?;
    assert!(
        CommandProcessingService::process(&processor, &who, &command)
            .await
            .result()
            .is_err()
    );
    assert!(matches!(
        processor.lookup(&who, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    policy.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(
        CommandProcessingService::process(&processor, &who, &command)
            .await
            .result()
            .as_ref()
            .unwrap_err()
            .authorization_error(),
        Some(&AuthorizationError::Unavailable)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(store.load(&stream()?).await?.is_empty());
    Ok(())
}

struct BrokenAudit {
    hang: bool,
    calls: AtomicUsize,
}
#[async_trait]
impl CommandAuditSink for BrokenAudit {
    async fn record(&self, _: &CommandAuditRecord) -> Result<(), rostfrei::AuditError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.hang {
            std::future::pending::<()>().await;
        }
        Err(rostfrei::AuditError::new("controlled audit sink failure"))
    }
}
#[tokio::test]
async fn failed_or_hung_started_and_finished_audit_do_not_change_command_success() -> TestResult {
    for hang in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let sink = Arc::new(BrokenAudit {
            hang,
            calls: AtomicUsize::new(0),
        });
        let mut processor =
            CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
                .authorizer(Arc::new(AllowAllCommands))
                .audit_sink(sink.clone())
                .audit_timeout(Duration::from_millis(20))
                .build()?;
        processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
        let who = caller("service", "tenant-a")?;
        let command = encoded("audit-failure", false)?;
        for origin in [CompletionOrigin::Committed, CompletionOrigin::Replayed] {
            let report = tokio::time::timeout(
                Duration::from_secs(2),
                CommandProcessingService::process(&processor, &who, &command),
            )
            .await?;
            assert!(matches!(report.audit(), AuditDelivery::Failed { .. }));
            assert_eq!(report.into_result()?.origin(), origin);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            sink.calls.load(Ordering::SeqCst),
            4,
            "both phases attempted, including replay"
        );
    }
    Ok(())
}

#[tokio::test]
async fn attempt_ids_phases_original_evidence_and_history_retention_are_independent() -> TestResult
{
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let audit = Arc::new(InMemoryCommandAuditSink::new(8));
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .authorizer(policy.clone())
        .audit_sink(audit.clone())
        .build()?;
    processor.register::<Change, _>(ChangeHandler(calls.clone()), map_rejection)?;
    let who = caller("verified-user", "tenant-a")?;
    let command = encoded("audit-stable-operation", false)?;
    let first = CommandProcessingService::process(&processor, &who, &command).await;
    let second = CommandProcessingService::process(&processor, &who, &command).await;
    policy.deny_read.store(true, Ordering::SeqCst);
    let denied = CommandProcessingService::process(&processor, &who, &command).await;
    assert_ne!(first.attempt_id(), second.attempt_id());
    assert_ne!(second.attempt_id(), denied.attempt_id());
    assert!(first.result().is_ok() && second.result().is_ok() && denied.result().is_err());
    let records = audit.records().await;
    assert_eq!(records.len(), 6);
    for report_id in [first.attempt_id(), second.attempt_id(), denied.attempt_id()] {
        let phases: Vec<_> = records
            .iter()
            .filter(|r| r.attempt_id() == report_id)
            .map(CommandAuditRecord::phase)
            .collect();
        assert_eq!(
            phases,
            [
                rostfrei::AuditPhase::Started,
                rostfrei::AuditPhase::Finished
            ]
        );
    }
    for record in &records {
        assert_eq!(record.command_id(), command.message_id());
        assert_eq!(record.original_payload(), command.payload());
        assert_eq!(record.principal(), who.principal());
    }
    for (id, terminal) in [
        (first.attempt_id(), rostfrei::AuditTerminal::Committed),
        (second.attempt_id(), rostfrei::AuditTerminal::Replayed),
        (denied.attempt_id(), rostfrei::AuditTerminal::Denied),
    ] {
        assert!(
            records
                .iter()
                .any(|record| record.attempt_id() == id && record.terminal() == Some(terminal))
        );
    }
    let before = store.load(&stream()?).await?;
    audit.clear().await;
    assert!(audit.records().await.is_empty());
    policy.deny_read.store(false, Ordering::SeqCst);
    let OutcomeLookup::Completed(replayed) =
        processor.lookup(&who, &command).await.into_result()?
    else {
        return Err("audit clearing erased completion".into());
    };
    assert_eq!(&replayed, first.result().as_ref().unwrap().outcome());
    assert_eq!(store.load(&stream()?).await?, before);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for _ in 0..5 {
        CommandProcessingService::process(&processor, &who, &command)
            .await
            .into_result()?;
    }
    assert!(
        audit.records().await.len() <= 8,
        "in-memory audit retention is bounded"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct RetrySpy(AtomicUsize);
impl ConflictRetryPolicy for RetrySpy {
    fn after_conflict(&self, _: ConflictRetry) -> ConflictRetryDecision {
        self.0.fetch_add(1, Ordering::SeqCst);
        ConflictRetryDecision::Stop
    }
}
#[tokio::test]
async fn retry_policy_sees_real_version_conflict_but_not_replay_identity_or_codec_failures()
-> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let retries = Arc::new(RetrySpy(AtomicUsize::new(0)));
    let make = || {
        CommandProcessor::builder(context().unwrap(), store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .conflict_retry_policy(retries.clone())
    };
    let who = caller("service", "scope")?;
    let mut racing = make().build()?;
    racing.register::<Change, _>(
        RaceOnce {
            store: store.clone(),
            observations: Arc::new(Mutex::new(Vec::new())),
        },
        map_rejection,
    )?;
    assert!(
        CommandProcessingService::process(&racing, &who, &encoded("real-conflict", true)?)
            .await
            .result()
            .is_err()
    );
    assert_eq!(retries.0.load(Ordering::SeqCst), 1);
    let mut ordinary = make().build()?;
    ordinary.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    let command = encoded("retry-classification", true)?;
    CommandProcessingService::process(&ordinary, &who, &command)
        .await
        .into_result()?;
    CommandProcessingService::process(&ordinary, &who, &command)
        .await
        .into_result()?;
    let changed = bus(baseline_processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
    )?)?
    .encode(CommandRequest::new(
        OperationId::new(command.operation_id().as_str())?,
        Change {
            amount: 99,
            reject: true,
        },
    ))?;
    assert!(
        CommandProcessingService::process(&ordinary, &who, &changed)
            .await
            .result()
            .is_err()
    );
    let mut broken = make()
        .rejection_codec(Arc::new(WireRejection {
            encodes: Arc::new(AtomicUsize::new(0)),
            decodes: Arc::new(AtomicUsize::new(0)),
            broken: true,
        }))
        .build()?;
    broken.register::<Change, _>(ChangeHandler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    assert!(
        CommandProcessingService::process(&broken, &who, &encoded("not-a-conflict", true)?)
            .await
            .result()
            .is_err()
    );
    assert_eq!(retries.0.load(Ordering::SeqCst), 1);
    Ok(())
}

/// Spy is an independent implementation of the two-method port, delegating decisions to a real service.
struct ServiceSpy {
    inner: Arc<dyn CommandProcessingService>,
    process: AtomicUsize,
    lookup: AtomicUsize,
}
#[async_trait]
impl CommandProcessingService for ServiceSpy {
    async fn process(
        &self,
        caller: &TrustedCommandContext,
        command: &EncodedCommand,
    ) -> rostfrei::ServiceReport<rostfrei::ProcessedCommand> {
        self.process.fetch_add(1, Ordering::SeqCst);
        self.inner.process(caller, command).await
    }
    async fn lookup(
        &self,
        caller: &TrustedCommandContext,
        command: &EncodedCommand,
    ) -> rostfrei::ServiceReport<OutcomeLookup<rostfrei_messaging_core::CommandResponse>> {
        self.lookup.fetch_add(1, Ordering::SeqCst);
        self.inner.lookup(caller, command).await
    }
}
#[tokio::test]
async fn local_adapter_accepts_an_independent_object_safe_service_and_audit_defaults_off()
-> TestResult {
    let real = Arc::new(processor(
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AllowAllCommands),
    )?);
    let who = caller("service", "tenant-a")?;
    let command = encoded("injected-service", false)?;
    assert!(matches!(
        real.lookup(&who, &command).await.audit(),
        AuditDelivery::Disabled
    ));
    let spy = Arc::new(ServiceSpy {
        inner: real,
        process: AtomicUsize::new(0),
        lookup: AtomicUsize::new(0),
    });
    let adapter = InMemoryMessagingAdapter::with_service(spy.clone());
    adapter.dispatch_as(&who, command.clone()).await?;
    let before = spy.process.load(Ordering::SeqCst) + spy.lookup.load(Ordering::SeqCst);
    adapter.dispatch_as(&who, command).await?;
    assert!(
        spy.process.load(Ordering::SeqCst) + spy.lookup.load(Ordering::SeqCst) > before,
        "cache must invoke the injected service"
    );
    assert!(
        CommandProcessor::builder(context()?, Arc::new(InMemoryEventStore::new()))
            .build()
            .is_err(),
        "new boundary requires an explicit authorization policy"
    );
    Ok(())
}

struct CountAuthority {
    inner: InMemoryEventStore,
    reads: Arc<AtomicUsize>,
}
#[async_trait]
impl EventHistory for CountAuthority {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl OutcomeReader for CountAuthority {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl CommandBackend for CountAuthority {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        self.inner.begin_command().await
    }
}
#[tokio::test]
async fn read_denial_precedes_even_the_authoritative_marker_lookup() -> TestResult {
    let reads = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    policy.deny_read.store(true, Ordering::SeqCst);
    let processor = CommandProcessor::builder(
        context()?,
        Arc::new(CountAuthority {
            inner: InMemoryEventStore::new(),
            reads: reads.clone(),
        }),
    )
    .authorizer(policy)
    .build()?;
    let who = caller("caller", "scope")?;
    let command = encoded("do-not-disclose-existence", false)?;
    assert!(
        CommandProcessingService::process(&processor, &who, &command)
            .await
            .result()
            .is_err()
    );
    assert!(processor.lookup(&who, &command).await.result().is_err());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn local_cached_response_cannot_resurrect_expired_details() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let service: Arc<dyn CommandProcessingService> = Arc::new(processor(
        store.clone(),
        calls.clone(),
        Arc::new(AllowAllCommands),
    )?);
    let adapter = InMemoryMessagingAdapter::with_service(service.clone());
    let who = caller("service", "scope")?;
    let command = encoded("local-expiry", true)?;
    adapter.dispatch_as(&who, command.clone()).await?;
    let OutcomeLookup::Completed(retained) = service.lookup(&who, &command).await.into_result()?
    else {
        return Err("missing local completion".into());
    };
    let key = OutcomeKey::new(
        store.authority().clone(),
        context()?.name().clone(),
        OperationId::new(command.operation_id().as_str())?,
    );
    store
        .expire_details(&key, retained.marker().revision())
        .await?;
    assert_eq!(
        adapter.dispatch_as(&who, command).await.unwrap_err().kind(),
        rostfrei::CommandBusErrorKind::CompletedDetailsExpired
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct Contender {
    reject: bool,
    calls: Arc<AtomicUsize>,
    barrier: Arc<tokio::sync::Barrier>,
}
#[async_trait]
impl CommandHandler<Change> for Contender {
    type Rejection = String;
    async fn handle(
        &self,
        _: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut counter = execution.load::<Counter>("one").await?;
        self.barrier.wait().await;
        if self.reject {
            Ok(CommandDecision::Rejected(
                "speculative loser or winner".to_owned(),
            ))
        } else {
            counter.raise(Added(7));
            Ok(CommandDecision::Accepted)
        }
    }
}
#[tokio::test]
async fn zero_budget_concurrent_opposite_proposals_reconcile_the_committed_winner() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut contenders = Vec::new();
    for reject in [false, true] {
        let mut p = CommandProcessor::builder(context()?, store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .conflict_retry_policy(Arc::new(BoundedConflictRetries::new(0)))
            .build()?;
        p.register::<Change, _>(
            Contender {
                reject,
                calls: calls.clone(),
                barrier: barrier.clone(),
            },
            map_rejection,
        )?;
        contenders.push(p);
    }
    let who = caller("service", "scope")?;
    let command = encoded("same-original-opposite-proposals", false)?;
    let (a, b) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            CommandProcessingService::process(&contenders[0], &who, &command),
            CommandProcessingService::process(&contenders[1], &who, &command)
        )
    })
    .await?;
    let (a, b) = (a.into_result()?, b.into_result()?);
    assert_eq!(a.outcome(), b.outcome());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let accepted = matches!(a.outcome().details(), OutcomeDetails::Available(response) if response.outcome() == &CommandResponseOutcome::Accepted);
    assert_eq!(store.load(&stream()?).await?.len(), usize::from(accepted));
    assert_eq!(
        CommandProcessingService::process(&contenders[0], &who, &command)
            .await
            .into_result()?
            .origin(),
        CompletionOrigin::Replayed
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "completed replay cannot enter the barrier or consume retry budget"
    );
    Ok(())
}

struct AlwaysRace {
    store: Arc<InMemoryEventStore>,
    calls: Arc<AtomicUsize>,
}

#[tokio::test]
async fn policy_enabled_processor_refuses_every_contextless_compatibility_path() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let processor = Arc::new(processor(store, calls.clone(), policy.clone())?);
    let command = encoded("compatibility-cannot-bypass-policy", false)?;
    assert!(processor.process(&command).await.is_err());
    assert!(processor.reconcile_completed(&command).await.is_err());
    let compatibility = bus(processor.clone())?;
    assert!(
        compatibility
            .dispatch_encoded(command.clone())
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(policy.seen.lock().unwrap().is_empty());
    CommandProcessingService::process(processor.as_ref(), &caller("verified", "scope")?, &command)
        .await
        .into_result()?;
    policy.deny_read.store(true, Ordering::SeqCst);
    assert!(processor.process(&command).await.is_err());
    assert!(processor.reconcile_completed(&command).await.is_err());
    assert!(compatibility.dispatch_encoded(command).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct InvalidAfterRead(RaceOnce);
#[async_trait]
impl CommandHandler<Change> for InvalidAfterRead {
    type Rejection = String;
    async fn handle(
        &self,
        command: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0.handle(command, execution).await?;
        Err(EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            "guarded framework refusal",
        )
        .into())
    }
}
#[tokio::test]
async fn framework_invalid_request_keeps_original_read_guards_through_retry() -> TestResult {
    for retries in [0, 1] {
        let store = Arc::new(InMemoryEventStore::new());
        let observations = Arc::new(Mutex::new(Vec::new()));
        let mut processor = CommandProcessor::builder(context()?, store.clone())
            .authorizer(Arc::new(AllowAllCommands))
            .conflict_retry_policy(Arc::new(BoundedConflictRetries::new(retries)))
            .build()?;
        processor.register::<Change, _>(
            InvalidAfterRead(RaceOnce {
                store: store.clone(),
                observations: observations.clone(),
            }),
            map_rejection,
        )?;
        let who = caller("service", "scope")?;
        let command = encoded("framework-refusal-guards", false)?;
        let result = CommandProcessingService::process(&processor, &who, &command)
            .await
            .into_result();
        if retries == 0 {
            assert!(result.is_err());
            assert_eq!(*observations.lock().unwrap(), [0]);
            assert!(matches!(
                processor.lookup(&who, &command).await.into_result()?,
                OutcomeLookup::Missing
            ));
        } else {
            let completed = result?;
            let response = completed.outcome().details().available()?;
            assert!(
                matches!(response.outcome(), CommandResponseOutcome::Rejected(rejection)
                if rejection.classification() == CommandRejectionClassification::InvalidRequest)
            );
            assert_eq!(*observations.lock().unwrap(), [0, 10]);
            assert_eq!(
                CommandProcessingService::process(&processor, &who, &command)
                    .await
                    .into_result()?
                    .origin(),
                CompletionOrigin::Replayed
            );
            assert_eq!(*observations.lock().unwrap(), [0, 10]);
        }
        assert_eq!(store.load(&stream()?).await?.len(), 1);
    }
    Ok(())
}
#[async_trait]
impl CommandHandler<Change> for AlwaysRace {
    type Rejection = String;
    async fn handle(
        &self,
        _: &Change,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let number = self.calls.fetch_add(1, Ordering::SeqCst);
        let loaded = execution.load::<Counter>("one").await?;
        let operation = OperationId::new(format!("competitor-{number}")).unwrap();
        let id = stream().unwrap();
        let commit = derive_commit_id(&id, &operation);
        let event = NewEvent::new(derive_event_id(&commit, 0), "added", 2, b"1".to_vec()).unwrap();
        let expected = if loaded.base_version().value() == 0 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(loaded.base_version())
        };
        self.store
            .append(
                &id,
                expected,
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(format!("competitor-{number}")),
                    vec![event],
                )
                .unwrap(),
            )
            .await?;
        Ok(CommandDecision::Rejected("stale read".to_owned()))
    }
}
#[tokio::test]
async fn processor_default_is_three_retries_after_the_initial_evaluation() -> TestResult {
    let store = Arc::new(InMemoryEventStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut processor = CommandProcessor::builder(context()?, store.clone())
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    processor.register::<Change, _>(
        AlwaysRace {
            store: store.clone(),
            calls: calls.clone(),
        },
        map_rejection,
    )?;
    let who = caller("service", "scope")?;
    let command = encoded("default-retries", true)?;
    assert!(
        CommandProcessingService::process(&processor, &who, &command)
            .await
            .result()
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        store.load(&stream()?).await?.len(),
        4,
        "only the four independently committed competitors exist"
    );
    assert!(matches!(
        processor.lookup(&who, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    Ok(())
}
