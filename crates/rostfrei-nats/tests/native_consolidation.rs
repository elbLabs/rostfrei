//! C1/C2/C3a boundary acceptance against the owned, authenticated real-NATS runner.
#![allow(
    clippy::panic_in_result_fn,
    reason = "acceptance assertions report invariant failures"
)]
#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "shared fixture parses owned NATS frames and locks test-only controls"
)]
mod modular_state_fault_support;
mod release_contract_support;

use async_trait::async_trait;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei_runtime::{
    CommandAccess, CommandProcessingService, CommandProcessorErrorKind, CompletionOrigin,
    PrincipalId, TrustedCommandContext,
};
use std::collections::BTreeMap;
rostfrei_runtime::install_macro_support!();

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CASE_TIMEOUT: Duration = Duration::from_secs(20);

// Reuse the contract fixture with small disposable namespaces, as in authority regressions.
async fn fixture() -> TestResult<(Fixture, Arc<NatsEventStore>)> {
    let application = ApplicationName::new(format!(
        "consolidation-{}-{}",
        std::process::id(),
        FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
    ))?;
    let context = application.bounded_context("release-contract")?;
    let messaging =
        ApplicationMessagingConfig::new(&application)?.with_max_bytes(4 * 1024 * 1024)?;
    let connection = connect(&NatsConnectionConfig::new(
        "native-consolidation",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let js = connection.jetstream().clone();
    provision_application_messaging(&js, &messaging).await?;
    let adapter = Arc::new(NatsMessagingAdapter::new(
        NatsPublisher::new(js.clone(), messaging.topology().clone()),
        NatsCommandResponseReader::new(js.clone(), messaging.topology().clone()),
    ));
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("consolidation")?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?;
    provision_event_store(&js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config).await?);
    Ok((
        Fixture {
            context,
            js,
            messaging,
            adapter,
        },
        store,
    ))
}

async fn proxied(store: &NatsEventStore) -> TestResult<(StateProxy, Arc<NatsEventStore>)> {
    let proxy = StateProxy::start_with_timeout(
        store.config().outcome_state_stream_name(),
        "unused",
        0,
        REQUEST_TIMEOUT,
    )
    .await?;
    let handle = Arc::new(NatsEventStore::connect(proxy.js.clone(), store.config().clone()).await?);
    Ok((proxy, handle))
}

fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("consolidation")?,
        SecurityScope::new("consolidation-scope")?,
    ))
}

#[derive(Default)]
struct AccessCounts {
    read: AtomicUsize,
    execute: AtomicUsize,
}
#[async_trait]
impl rostfrei_runtime::CommandAuthorizer for AccessCounts {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei_runtime::ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), rostfrei_runtime::AuthorizationError> {
        match access {
            CommandAccess::ReadOutcome => self.read.fetch_add(1, Ordering::SeqCst),
            CommandAccess::Execute => self.execute.fetch_add(1, Ordering::SeqCst),
        };
        Ok(())
    }
}

fn service(
    f: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
    access: Arc<AccessCounts>,
) -> TestResult<Arc<dyn CommandProcessingService>> {
    let mut processor = CommandProcessor::builder(f.context.clone(), store)
        .authorizer(access)
        .build()?;
    processor.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(Arc::new(processor))
}

fn key(store: &NatsEventStore, f: &Fixture, operation: &str) -> TestResult<OutcomeKey> {
    Ok(OutcomeKey::new(
        store.authority().clone(),
        f.context.name().clone(),
        OperationId::new(operation)?,
    ))
}

async fn stream_snapshot(f: &Fixture, name: &str) -> TestResult<serde_json::Value> {
    let stream = match f.js.get_stream(name).await {
        Ok(stream) => stream,
        Err(error) => {
            assert!(matches!(
                error.kind(),
                async_nats::jetstream::context::GetStreamErrorKind::JetStream(ref error)
                    if error.code() == 404
            ));
            return Ok(serde_json::json!({"missing": name}));
        }
    };
    let info = stream.cached_info();
    let mut records = Vec::new();
    if info.state.messages != 0 {
        for sequence in info.state.first_sequence..=info.state.last_sequence {
            let message = stream.get_raw_message(sequence).await?;
            records.push(serde_json::json!({
                "sequence": message.sequence,
                "subject": message.subject.as_str(),
                "payload": message.payload.to_vec(),
            }));
        }
    }
    Ok(serde_json::json!({
        "created": info.created.to_string(), "config": info.config,
        "messages": info.state.messages, "last": info.state.last_sequence,
        "records": records,
    }))
}

async fn snapshot(f: &Fixture, store: &NatsEventStore) -> TestResult<serde_json::Value> {
    Ok(serde_json::json!([
        stream_snapshot(f, store.config().stream_name()).await?,
        stream_snapshot(f, store.config().outcome_state_stream_name()).await?,
    ]))
}

async fn seed(f: &Fixture, store: &NatsEventStore) -> TestResult {
    let operation = OperationId::new("retained-control")?;
    let fingerprint = ContentFingerprint::digest("retained-control");
    let evidence = CommandExecutionMetadata::new(operation.clone(), fingerprint)
        .with_default_outcome_evidence::<()>()
        .outcome_evidence()
        .cloned();
    store
        .begin_command()
        .await?
        .commit_outcome(
            EventTransaction::new(operation, fingerprint, vec![])
                .with_bounded_context(f.context.name().clone())
                .with_completion(CommandCompletion::new(evidence, None)),
        )
        .await?;
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one gated absence boundary across three real authority faults"
)]
async fn missing_receipt_after_verified_journal_cannot_authorize_a_damaged_pair() -> TestResult {
    for damage in ["state-config", "state-recreated", "pair-recreated"] {
        let (f, independent) = fixture().await?;
        seed(&f, &independent).await?;
        let (proxy, store) = proxied(&independent).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let access = Arc::new(AccessCounts::default());
        let service = service(&f, store.clone(), calls.clone(), access.clone())?;
        let who = caller()?;
        let command = CommandBus::new(f.context.clone(), f.adapter.clone())
            .encode(request("absent", "target", "create")?)?;
        let mut gate = proxy.gate_request(
            format!("$JS.API.STREAM.MSG.GET.{}", store.config().stream_name()),
            1,
        )?;
        let submit = async { TestResult::Ok(service.process(&who, &command).await.into_result()) };
        let fault = async {
            gate.wait().await?;
            assert_eq!(access.read.load(Ordering::SeqCst), 1);
            assert_eq!(access.execute.load(Ordering::SeqCst), 0);
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            let state_name = store.config().outcome_state_stream_name();
            let old = f.js.get_stream(state_name).await?;
            match damage {
                "state-config" => {
                    let mut changed = old.cached_info().config.clone();
                    changed.allow_direct = true;
                    f.js.update_stream(changed).await?;
                }
                "state-recreated" => {
                    f.js.delete_stream(state_name).await?;
                    let replacement = f.js.create_stream(old.cached_info().config.clone()).await?;
                    assert_ne!(old.cached_info().created, replacement.cached_info().created);
                }
                _ => {
                    f.js.delete_stream(store.config().stream_name()).await?;
                    f.js.delete_stream(state_name).await?;
                    provision_event_store(&f.js, store.config()).await?;
                    let replacement =
                        NatsEventStore::connect(f.js.clone(), store.config().clone()).await?;
                    assert_ne!(store.authority(), replacement.authority());
                    seed(&f, &replacement).await?;
                }
            }
            let before = snapshot(&f, &store).await?;
            gate.release()?;
            TestResult::Ok(before)
        };
        let (result, before) =
            tokio::time::timeout(CASE_TIMEOUT, async { tokio::try_join!(submit, fault) }).await??;
        let Err(error) = result else {
            return Err("damaged Missing authorized execution".into());
        };
        assert_eq!(
            error.kind(),
            CommandProcessorErrorKind::CompletedDetailsBlocked,
            "{damage}"
        );
        let Err(error) = store.lookup_outcome(&key(&store, &f, "absent")?).await else {
            return Err("damaged authority reported executable absence".into());
        };
        assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
        assert_eq!(error.origin(), EventStoreErrorOrigin::Backend);
        assert_eq!(access.execute.load(Ordering::SeqCst), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(snapshot(&f, &store).await?, before, "{damage}");
    }
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one final-read expiry race with independent storage and replay witnesses"
)]
async fn expiry_before_final_state_get_returns_completed_expired_without_reevaluation() -> TestResult
{
    let (f, independent) = fixture().await?;
    let (proxy, store) = proxied(&independent).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let access = Arc::new(AccessCounts::default());
    let service = service(&f, store.clone(), calls.clone(), access.clone())?;
    let who = caller()?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "expire-final",
        "target",
        "create",
    )?)?;
    // GET #1 reads back Active preparation. GET #2 is the retained final lifecycle read,
    // after the journal PubAck and publication verification, on baseline and candidate.
    let mut gate = proxy.gate_request(
        format!(
            "$JS.API.STREAM.MSG.GET.{}",
            store.config().outcome_state_stream_name()
        ),
        2,
    )?;
    let submit = async { TestResult::Ok(service.process(&who, &command).await.into_result()) };
    let fault = async {
        gate.wait().await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.messages(store.config().stream_name()).await?, 2);
        let outcome_key = key(&independent, &f, "expire-final")?;
        let OutcomeLookup::Completed(winner) = independent.lookup_outcome(&outcome_key).await?
        else {
            return Err("gate was reached before a durable decision".into());
        };
        assert!(matches!(winner.details(), OutcomeDetails::Available(_)));
        let journal = stream_snapshot(&f, store.config().stream_name()).await?;
        let expiry = independent
            .expire_details(&outcome_key, winner.marker().revision())
            .await?;
        assert_eq!(expiry.reclamation(), ReclamationStatus::Reclaimed);
        assert_eq!(
            stream_snapshot(&f, store.config().stream_name()).await?,
            journal
        );
        assert_eq!(
            f.messages(store.config().outcome_state_stream_name())
                .await?,
            1
        );
        let before = snapshot(&f, &store).await?;
        gate.release()?;
        TestResult::Ok((winner.marker().clone(), before))
    };
    let (result, (marker, before)) =
        tokio::time::timeout(CASE_TIMEOUT, async { tokio::try_join!(submit, fault) }).await??;
    let completed = result?;
    assert_eq!(completed.origin(), CompletionOrigin::Committed);
    assert_eq!(completed.outcome().marker(), &marker);
    assert!(matches!(
        completed.outcome().details(),
        OutcomeDetails::Expired
    ));
    let Err(error) = completed.admit(&who, &command) else {
        return Err("expired completion disclosed a business response".into());
    };
    assert_eq!(
        error.kind(),
        CommandProcessorErrorKind::CompletedDetailsExpired
    );
    let replay = service.process(&who, &command).await.into_result()?;
    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
    assert_eq!(replay.outcome(), completed.outcome());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(access.execute.load(Ordering::SeqCst), 1);
    let history = independent.load(&stream("target")?).await?;
    assert_eq!(history.len(), 1);
    assert_eq!(
        independent.audit_history(&stream("target")?).await?,
        history
    );
    assert_eq!(snapshot(&f, &store).await?, before);
    Ok(())
}

struct AcceptNoRead(Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<()> for AcceptNoRead {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::test]
async fn state_loss_at_final_get_keeps_recovery_origin_and_never_reexecutes() -> TestResult {
    let (f, independent) = fixture().await?;
    let (proxy, store) = proxied(&independent).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = AcceptNoRead(calls.clone());
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("lost-final-state")?,
        ContentFingerprint::digest("lost-final-state"),
    )
    .with_bounded_context(f.context.name().clone());
    let codec = JsonRejectionCodec::new("consolidation", 1);
    let executor = CommandExecutor::new(store.clone()).with_max_conflict_retries(0);
    let mut gate = proxy.gate_request(
        format!(
            "$JS.API.STREAM.MSG.GET.{}",
            store.config().outcome_state_stream_name()
        ),
        2,
    )?;
    let submit = async {
        TestResult::Ok(
            executor
                .execute_completed_with_rejection_codec(&handler, metadata.clone(), &(), &codec)
                .await,
        )
    };
    let fault = async {
        gate.wait().await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.messages(store.config().stream_name()).await?, 1);
        assert_eq!(
            f.messages(store.config().outcome_state_stream_name())
                .await?,
            1
        );
        assert!(matches!(
            independent
                .lookup_outcome(&key(&independent, &f, "lost-final-state")?)
                .await?,
            OutcomeLookup::Completed(_)
        ));
        f.js.delete_stream(store.config().outcome_state_stream_name())
            .await?;
        let before = snapshot(&f, &store).await?;
        gate.release()?;
        TestResult::Ok(before)
    };
    let (result, before) =
        tokio::time::timeout(CASE_TIMEOUT, async { tokio::try_join!(submit, fault) }).await??;
    let Err(CommandExecutionError::Store(error)) = result else {
        return Err("lost final state did not return a store recovery failure".into());
    };
    assert_eq!(error.kind(), EventStoreErrorKind::Unavailable);
    assert_eq!(error.origin(), EventStoreErrorOrigin::OutcomeRecovery);
    let retry = executor
        .execute_completed_with_rejection_codec(&handler, metadata, &(), &codec)
        .await;
    let Err(CommandExecutionError::Store(error)) = retry else {
        return Err("durable decision with missing state became executable".into());
    };
    assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
    assert_eq!(error.origin(), EventStoreErrorOrigin::OutcomeRecovery);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot(&f, &store).await?, before);
    Ok(())
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Work {
    info: usize,
    get: usize,
    create: usize,
    pull: usize,
    delete: usize,
    publish: usize,
    other: usize,
}
fn work(counts: &BTreeMap<String, usize>, prefix: &str) -> TestResult<Work> {
    let mut work = Work::default();
    for (subject, count) in counts {
        let field = if subject.starts_with("$JS.API.STREAM.INFO.") {
            &mut work.info
        } else if subject.starts_with("$JS.API.STREAM.MSG.GET.") {
            &mut work.get
        } else if subject.starts_with("$JS.API.CONSUMER.CREATE.") {
            &mut work.create
        } else if subject.starts_with("$JS.API.CONSUMER.MSG.NEXT.") {
            &mut work.pull
        } else if subject.starts_with("$JS.API.CONSUMER.DELETE.") {
            &mut work.delete
        } else if subject.starts_with(prefix) {
            &mut work.publish
        } else {
            &mut work.other
        };
        *field = field
            .checked_add(*count)
            .ok_or("request classification overflow")?;
    }
    Ok(work)
}

#[allow(
    clippy::too_many_lines,
    reason = "fresh and replay wire budgets share handler, authority and retained-record guards"
)]
async fn request_work(one_event: bool) -> TestResult {
    let (f, independent) = fixture().await?;
    let (proxy, store) = proxied(&independent).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let access = Arc::new(AccessCounts::default());
    let service = service(&f, store.clone(), calls.clone(), access.clone())?;
    let who = caller()?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "measured",
        "target",
        if one_event { "create" } else { "noop" },
    )?)?;
    proxy.start_counting();
    let completed = service.process(&who, &command).await.into_result()?;
    let fresh_trace = proxy.finish_counting();
    assert_eq!(completed.origin(), CompletionOrigin::Committed);
    assert_eq!(
        completed.admit(&who, &command)?.outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let before = snapshot(&f, &store).await?;
    proxy.start_counting();
    let replay = service.process(&who, &command).await.into_result()?;
    let replay_trace = proxy.finish_counting();
    assert_eq!(replay.origin(), CompletionOrigin::Replayed);
    assert_eq!(replay.outcome(), completed.outcome());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(access.read.load(Ordering::SeqCst), 2);
    assert_eq!(access.execute.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.messages(store.config().stream_name()).await?,
        if one_event { 2 } else { 1 }
    );
    assert_eq!(
        f.messages(store.config().outcome_state_stream_name())
            .await?,
        1
    );
    assert_eq!(snapshot(&f, &store).await?, before);
    let history = independent.load(&stream("target")?).await?;
    assert_eq!(history.len(), usize::from(one_event));
    assert_eq!(
        independent.audit_history(&stream("target")?).await?,
        history
    );
    let receipt = completed.outcome().marker().integrity_receipt();
    assert_eq!(receipt.streams().len(), usize::from(one_event));
    if one_event {
        let participant = receipt
            .streams()
            .first()
            .ok_or("event participant missing")?;
        assert_eq!(participant.base_version(), StreamVersion::ZERO);
        assert_eq!(participant.events(), history.as_slice());
        let event = history.first().ok_or("committed event missing")?;
        assert_eq!(event.operation_id().as_str(), "measured");
        assert!(serde_json::from_slice::<Changed>(event.payload())?.created);
    }
    let fresh = work(&fresh_trace, store.config().subject_prefix())?;
    let replay = work(&replay_trace, store.config().subject_prefix())?;
    eprintln!("consolidation one_event={one_event}: fresh={fresh:?}, replay={replay:?}");
    eprintln!(
        "consolidation subjects: {}",
        serde_json::to_string(&serde_json::json!({
            "fresh": fresh_trace, "replay": replay_trace,
        }))?
    );
    // Exact candidate budgets: C1 + narrow C2 + C3a, with the full final lookup retained.
    let (expected_fresh, expected_replay) = if one_event {
        (
            Work {
                info: 24,
                get: 10,
                create: 1,
                pull: 1,
                delete: 1,
                publish: 3,
                other: 0,
            },
            Work {
                info: 4,
                get: 3,
                create: 1,
                pull: 1,
                delete: 1,
                ..Work::default()
            },
        )
    } else {
        (
            Work {
                info: 20,
                get: 7,
                publish: 2,
                ..Work::default()
            },
            Work {
                info: 3,
                get: 2,
                ..Work::default()
            },
        )
    };
    assert_eq!(fresh, expected_fresh);
    assert_eq!(replay, expected_replay);
    Ok(())
}

#[tokio::test]
async fn request_work_noevent_and_replay_preserves_one_decision() -> TestResult {
    request_work(false).await
}

#[tokio::test]
async fn request_work_oneevent_and_replay_preserves_verified_event() -> TestResult {
    request_work(true).await
}
