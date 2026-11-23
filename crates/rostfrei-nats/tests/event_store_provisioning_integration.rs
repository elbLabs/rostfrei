mod modular_restart_support;
mod native_connection_support;

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream::{self, stream};
use futures_util::StreamExt as _;
use rostfrei_core::{
    AggregateId, AggregateType, CommandCompletion, CommandExecutionMetadata, ContentFingerprint,
    EncodedRejection, EventBatch, EventStoreErrorKind, EventTransaction, ExecutionStore,
    ExpectedVersion, NewEvent, OperationId, OutcomeKey, OutcomeLifecycle, OutcomeLifecycleError,
    OutcomeLookup, OutcomeReader, StreamId, TransactionParticipant, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsEventStore, NatsEventStoreConfig, provision_event_store, update_event_store,
};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

async fn fixture() -> TestResult<(jetstream::Context, NatsEventStoreConfig)> {
    let client =
        native_connection_support::client(rostfrei_testing::integration::nats_url()?).await?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let context = ApplicationName::new(format!("policy-test-{nanos}-{sequence}"))?
        .bounded_context("policy")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(32 * 1024 * 1024, 512 * 1024)?;
    Ok((jetstream::new(client), config))
}

fn decision(
    config: &NatsEventStoreConfig,
    operation: &str,
    create: Option<&StreamId>,
    rejection: Option<EncodedRejection>,
) -> TestResult<EventTransaction> {
    let fingerprint = ContentFingerprint::digest(operation);
    let operation = OperationId::new(operation)?;
    let evidence = CommandExecutionMetadata::new(operation.clone(), fingerprint)
        .with_default_outcome_evidence::<()>();
    let mut participants = vec![];
    if let Some(stream) = create {
        let commit = derive_commit_id(stream, &operation);
        let event = NewEvent::new(derive_event_id(&commit, 0), "created", 1, b"{}".to_vec())?;
        let batch = EventBatch::new(commit, operation.clone(), fingerprint, vec![event])?;
        participants.push(TransactionParticipant::new(
            stream.clone(),
            ExpectedVersion::NoStream,
            Some(batch),
        ));
    }
    Ok(EventTransaction::new(operation, fingerprint, participants)
        .with_bounded_context(config.bounded_context().clone())
        .with_completion(CommandCompletion::new(
            evidence.outcome_evidence().cloned(),
            rejection,
        )))
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn split_authority_uses_readable_names_and_retains_results_after_reconnect() -> TestResult<()>
{
    let (context, config) = fixture().await?;
    let prefix = config
        .application()
        .as_str()
        .replace('-', "_")
        .to_uppercase();
    let journal_name = format!("{prefix}__POLICY__JOURNAL");
    let results_name = format!("{prefix}__POLICY__COMMAND_RESULTS");
    let original_subject_prefix = config.subject_prefix().to_owned();
    let config = config.with_fresh_outcomes()?;
    provision_event_store(&context, &config).await?;
    assert_eq!(config.stream_name(), journal_name);
    assert_eq!(config.outcome_state_stream_name(), results_name);
    assert_eq!(config.subject_prefix(), original_subject_prefix);
    assert_eq!(config.completed_outcome_epoch(), None);
    for name in [&journal_name, &results_name] {
        let stream = context.get_stream(name).await?;
        assert_eq!(stream.cached_info().config.name, *name);
        assert!(stream.cached_info().config.max_age.is_zero());
        assert!(
            !stream
                .cached_info()
                .config
                .metadata
                .contains_key("rostfrei.completed-outcomes.epoch")
        );
        assert_eq!(
            stream
                .cached_info()
                .config
                .metadata
                .get("rostfrei.completed-outcomes.version")
                .map(String::as_str),
            Some("2")
        );
    }

    let proposal = decision(&config, "remembered-noop", None, None)?;
    let store = NatsEventStore::connect(context.clone(), config.clone()).await?;
    let original = store
        .begin_command()
        .await?
        .commit_outcome(proposal.clone())
        .await?;
    let marker = original.outcome().marker();
    assert!(matches!(
        store
            .expire_details(marker.detail_reference().key(), marker.revision())
            .await,
        Err(OutcomeLifecycleError::Unsupported)
    ));
    drop(store);

    let before = split_info(&context, &config).await?;
    let incompatible = config
        .clone()
        .with_evicting_result_bodies(config.max_stream_bytes())?;
    for attempt in [
        provision_event_store(&context, &incompatible).await,
        update_event_store(&context, &incompatible).await,
        NatsEventStore::connect(context.clone(), incompatible)
            .await
            .map(|_| ()),
    ] {
        assert_eq!(
            attempt.unwrap_err().kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
    }
    assert_eq!(split_info(&context, &config).await?, before);
    provision_event_store(&context, &config).await?;
    let reconnected = NatsEventStore::connect(context.clone(), config).await?;
    let replay = reconnected
        .begin_command()
        .await?
        .commit_outcome(proposal)
        .await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.outcome(), original.outcome());
    for name in [&journal_name, &results_name] {
        assert_eq!(
            context.get_stream(name).await?.cached_info().state.messages,
            1
        );
        context.delete_stream(name).await?;
    }
    Ok(())
}

async fn split_update_fixture() -> TestResult<(
    jetstream::Context,
    NatsEventStoreConfig,
    NatsEventStoreConfig,
)> {
    let (js, config) = fixture().await?;
    let config = config
        .with_fresh_outcomes()?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?;
    provision_event_store(&js, &config).await?;
    let target = config
        .clone()
        .with_storage_limits(4 * 1024 * 1024, config.max_event_bytes())?;
    Ok((js, config, target))
}

async fn split_info(
    js: &jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<[stream::Info; 2]> {
    let journal = js.get_stream(config.stream_name()).await?;
    let results = js.get_stream(config.outcome_state_stream_name()).await?;
    Ok([journal.cached_info().clone(), results.cached_info().clone()])
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn full_journal_refuses_another_decision_without_displacing_history() -> TestResult<()> {
    let (js, config) = fixture().await?;
    let config = config
        .with_fresh_outcomes()?
        .with_storage_limits(16 * 1024, 8 * 1024)?
        .with_evicting_result_bodies(32 * 1024)?;
    provision_event_store(&js, &config).await?;
    let store = NatsEventStore::connect(js.clone(), config.clone()).await?;
    let id = StreamId::new(AggregateType::new("Order")?, AggregateId::new("first")?);
    let mut winners = Vec::new();
    for i in 0..32 {
        let operation = format!("journal-{i}");
        let proposal = decision(&config, &operation, (i == 0).then_some(&id), None)?;
        let result = store.begin_command().await?.commit_outcome(proposal).await;
        match result {
            Ok(commit) => winners.push(commit.outcome().marker().clone()),
            Err(error) => {
                assert_eq!(
                    error.kind(),
                    EventStoreErrorKind::CapacityExhausted,
                    "{error}"
                );
                assert!(!winners.is_empty());
                let key = OutcomeKey::new(
                    store.authority().clone(),
                    config.bounded_context().clone(),
                    OperationId::new(operation)?,
                );
                assert_eq!(store.lookup_outcome(&key).await?, OutcomeLookup::Missing);
                for marker in winners {
                    let OutcomeLookup::Completed(winner) = store
                        .lookup_outcome(marker.detail_reference().key())
                        .await?
                    else {
                        return Err("journal lost an earlier decision".into());
                    };
                    assert_eq!(winner.marker(), &marker);
                }
                assert_eq!(store.audit_history(&id).await?.len(), 1);
                for info in split_info(&js, &config).await? {
                    js.delete_stream(info.config.name).await?;
                }
                return Ok(());
            }
        }
    }
    Err("journal did not reach its byte limit".into())
}

#[tokio::test]
async fn split_capacity_maintenance_preserves_original_authority_across_restarts() -> TestResult<()>
{
    Box::pin(resize_across_restarts(
        "split_capacity_maintenance_preserves_original_authority_across_restarts",
        false,
    ))
    .await
}

#[tokio::test]
async fn split_capacity_maintenance_resumes_partial_layout_three_growth() -> TestResult<()> {
    Box::pin(resize_across_restarts(
        "split_capacity_maintenance_resumes_partial_layout_three_growth",
        true,
    ))
    .await
}

#[allow(clippy::panic_in_result_fn)]
async fn resize_across_restarts(test: &str, partial: bool) -> TestResult<()> {
    let Ok(phase) = std::env::var("ROSTFREI_MODULAR_RESTART_MODE") else {
        return modular_restart_support::run(test, &["seed", "maintenance", "replay"]).await;
    };
    let js = jetstream::new(
        async_nats::ConnectOptions::with_user_and_password(
            std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
            std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
        )
        .connect(std::env::var("ROSTFREI_NATS_URL")?)
        .await?,
    );
    let app = ApplicationName::new(std::env::var("ROSTFREI_MODULAR_RESTART_APP")?)?;
    let mut config = NatsEventStoreConfig::for_bounded_context(&app.bounded_context("policy")?)?
        .with_fresh_outcomes()?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?;
    if partial {
        config = config.with_evicting_result_bodies(512 * 1024)?;
    }
    let mut target = config
        .clone()
        .with_storage_limits(4 * 1024 * 1024, config.max_event_bytes())?;
    if partial {
        target = target.with_evicting_result_bodies(1024 * 1024)?;
        if phase != "seed" {
            config =
                config.with_storage_limits(target.max_stream_bytes(), target.max_event_bytes())?;
        }
    }
    let records = std::path::PathBuf::from(std::env::var("ROSTFREI_MODULAR_RESTART_RECORDS")?);
    // The default workspace fixture is 2.12.15. In the upgrade qualification
    // the explicit seed binary provisions that same store for the newer broker.
    let fixed = std::env::var_os("ROSTFREI_CONTRACT_SEED_NATS_BINARY").is_some();
    if phase == "seed" {
        provision_event_store(&js, &config).await?;
    } else if phase == "maintenance" {
        assert_resize(&js, &config, &target, fixed).await?;
    }
    let [journal, results] = split_info(&js, &config).await?;
    let identity = format!("{}\n{}", journal.created, results.created);
    assert_original(
        &phase,
        &records.join("original-identity"),
        identity.as_bytes(),
    )?;
    let config = if phase == "seed" || !fixed {
        config
    } else {
        target
    };
    let store = NatsEventStore::connect(js.clone(), config.clone()).await?;
    let rejection = EncodedRejection {
        codec_id: "policy".into(),
        codec_version: 1,
        bytes: b"original-rejection".to_vec(),
    };
    let id = StreamId::new(AggregateType::new("Order")?, AggregateId::new("one")?);
    for (name, event, reason) in [
        ("accepted", Some(&id), None),
        ("rejected", None, Some(rejection)),
    ] {
        let commit = store
            .begin_command()
            .await?
            .commit_outcome(decision(&config, name, event, reason.clone())?)
            .await?;
        assert_eq!(commit.is_exact_replay(), phase != "seed");
        assert_original(
            &phase,
            &records.join(name),
            &commit.outcome().marker().encode_manifest()?,
        )?;
        let rostfrei_core::OutcomeDetails::Available(body) = commit.outcome().details() else {
            return Err("lost original body".into());
        };
        assert_eq!(body.rejection(), reason.as_ref());
    }
    assert_eq!(store.audit_history(&id).await?.len(), 1);
    assert_eq!(split_info(&js, &config).await?[1].state.messages, 2);
    if partial && phase == "seed" {
        // Inject the intermediate state before the first restart, while it is
        // still the original file store. Only the journal has reached target.
        let mut policy = journal.config;
        policy.max_bytes = 4 * 1024 * 1024;
        js.update_stream(policy).await?;
    }
    if phase == "replay" {
        let new = store
            .begin_command()
            .await?
            .commit_outcome(decision(&config, "new", None, None)?)
            .await?;
        assert!(!new.is_exact_replay());
    }
    Ok(())
}

#[allow(clippy::panic_in_result_fn)]
async fn assert_resize(
    js: &jetstream::Context,
    config: &NatsEventStoreConfig,
    target: &NatsEventStoreConfig,
    fixed: bool,
) -> TestResult<()> {
    let before = split_info(js, config).await?;
    let result = update_event_store(js, target).await;
    if fixed {
        result?;
        let [mut journal, mut results] = before;
        journal.config.max_bytes = target.max_stream_bytes();
        results.config.max_bytes = target.max_result_stream_bytes();
        assert_eq!(
            split_info(js, target).await?,
            [journal, results],
            "only max_bytes may change"
        );
    } else {
        let Err(error) = result else {
            return Err("unsafe growth reported success".into());
        };
        assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
        assert!(error.message().contains("identity-preserving resize"));
        assert_eq!(split_info(js, config).await?, before);
        update_event_store(js, config).await?; // verified no-op needs no upgrade
    }
    Ok(())
}

fn assert_original(phase: &str, path: &std::path::Path, observed: &[u8]) -> TestResult<()> {
    if phase == "seed" {
        std::fs::write(path, observed)?;
    } else if std::fs::read(path)? != observed {
        return Err(format!("{phase}: original {} changed", path.display()).into());
    }
    Ok(())
}

#[tokio::test]
async fn split_update_refuses_to_shrink_results_before_growing_journal() -> TestResult<()> {
    refused_split_update(|results| results.max_bytes = 8 * 1024 * 1024).await
}

#[tokio::test]
async fn split_update_refuses_mismatched_companion_before_growing_either_stream() -> TestResult<()>
{
    refused_split_update(|results| {
        results.metadata.insert(
            "rostfrei.split.journal-incarnation".to_owned(),
            "other-journal".to_owned(),
        );
    })
    .await
}

#[allow(clippy::panic_in_result_fn)]
async fn refused_split_update(edit_results: impl FnOnce(&mut stream::Config)) -> TestResult<()> {
    let (js, config, target) = split_update_fixture().await?;
    let [_, mut results] = split_info(&js, &config).await?;
    edit_results(&mut results.config);
    js.update_stream(results.config).await?;
    let before = split_info(&js, &config).await?;
    let Err(error) = update_event_store(&js, &target).await else {
        return Err("invalid pair passed preflight".into());
    };
    assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
    assert_eq!(split_info(&js, &config).await?, before);
    for info in before {
        js.delete_stream(info.config.name).await?;
    }
    Ok(())
}

#[tokio::test]
async fn old_subject_layouts_require_a_fresh_namespace_without_mutating_storage() {
    for (filter, subject) in [
        ("*", "opaque-digest"),
        ("*.*.*.*", "Type.id.correlation.causation"),
    ] {
        for split in [false, true] {
            for populated in [false, true] {
                let (context, mut config) = fixture().await.expect("layout fixture");
                if split {
                    config = config
                        .with_fresh_split_outcome_epoch("layout-fixture")
                        .expect("split configuration");
                }
                let mut old_policy = config.stream_config();
                old_policy.subjects = vec![
                    format!("{}.aggregate.{filter}", config.subject_prefix()),
                    config.transaction_subject_filter(),
                ];
                context
                    .create_stream(old_policy)
                    .await
                    .expect("old subject layout");
                if populated {
                    context
                        .publish(
                            format!("{}.aggregate.{subject}", config.subject_prefix()),
                            b"retained old history".to_vec().into(),
                        )
                        .await
                        .expect("publish old fact")
                        .await
                        .expect("old fact acknowledgement");
                }
                let before = context
                    .get_stream(config.stream_name())
                    .await
                    .expect("old journal")
                    .cached_info()
                    .clone();
                let connected = NatsEventStore::connect(context.clone(), config.clone())
                    .await
                    .err()
                    .expect("old layout must not connect");
                for error in [
                    connected,
                    provision_event_store(&context, &config)
                        .await
                        .expect_err("old layout must not reprovision"),
                    update_event_store(&context, &config)
                        .await
                        .expect_err("old layout must not be upgraded in place"),
                ] {
                    assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
                    assert!(
                        error.message().contains("incompatible subject layout"),
                        "{error}"
                    );
                    assert!(error.message().contains("fresh namespace"), "{error}");
                }
                let after = context
                    .get_stream(config.stream_name())
                    .await
                    .expect("preserved old journal");
                assert_eq!(after.cached_info().created, before.created);
                assert_eq!(after.cached_info().config, before.config);
                assert_eq!(after.cached_info().state.messages, before.state.messages);
                assert_eq!(
                    after.cached_info().state.last_sequence,
                    before.state.last_sequence
                );
                if populated {
                    assert_eq!(
                        after
                            .get_raw_message(1)
                            .await
                            .expect("retained old fact")
                            .payload
                            .as_ref(),
                        b"retained old history"
                    );
                }
                if split {
                    assert!(
                        context
                            .get_stream(config.outcome_state_stream_name())
                            .await
                            .is_err(),
                        "rejected provisioning created an outcome state stream"
                    );
                }
                context
                    .delete_stream(config.stream_name())
                    .await
                    .expect("cleanup old journal");
            }
        }
    }
}

#[tokio::test]
async fn provisioning_requires_explicit_policy_changes_and_preserves_history() {
    let (context, config) = fixture().await.expect("policy fixture");
    let missing = NatsEventStore::connect(context.clone(), config.clone()).await;
    assert!(matches!(missing, Err(error) if error.kind() == EventStoreErrorKind::Unavailable));
    assert!(context.get_stream(config.stream_name()).await.is_err());
    provision_event_store(&context, &config)
        .await
        .expect("create stream");
    context
        .publish(
            config.transaction_guard_subject("history", 0),
            br"{}".to_vec().into(),
        )
        .await
        .expect("publish history")
        .await
        .expect("history acknowledgement");
    let before = context
        .get_stream(config.stream_name())
        .await
        .expect("existing stream")
        .cached_info()
        .clone();
    provision_event_store(&context, &config)
        .await
        .expect("verify existing stream");

    for capacity in [16 * 1024 * 1024, 64 * 1024 * 1024] {
        let changed = config
            .clone()
            .with_storage_limits(capacity, config.max_event_bytes())
            .expect("changed capacity");
        let error = provision_event_store(&context, &changed)
            .await
            .expect_err("provisioning must not resize the stream");
        assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
        assert!(error.message().contains("max_bytes"));
        let actual = context
            .get_stream(config.stream_name())
            .await
            .expect("unmodified stream");
        assert_eq!(actual.cached_info().config, before.config);
        assert_eq!(actual.cached_info().state.messages, before.state.messages);
        assert_eq!(
            actual.cached_info().state.last_sequence,
            before.state.last_sequence
        );
    }

    let lower_write_limit = config
        .clone()
        .with_storage_limits(config.max_stream_bytes(), 64 * 1024)
        .expect("lower application write limit");
    provision_event_store(&context, &lower_write_limit)
        .await
        .expect("larger historical message capacity is compatible");
    NatsEventStore::connect(context.clone(), lower_write_limit)
        .await
        .expect("verification-only startup");
    assert_eq!(
        context
            .get_stream(config.stream_name())
            .await
            .expect("preserved policy")
            .cached_info()
            .config,
        before.config
    );

    let reduced = config
        .clone()
        .with_storage_limits(16 * 1024 * 1024, 64 * 1024)
        .expect("explicit reduced policy");
    update_event_store(&context, &reduced)
        .await
        .expect("explicit capacity reduction");
    let after = context
        .get_stream(config.stream_name())
        .await
        .expect("updated stream");
    assert_eq!(
        after.cached_info().config.max_bytes,
        reduced.max_stream_bytes()
    );
    assert_eq!(
        after.cached_info().config.max_message_size,
        before.config.max_message_size
    );
    assert_eq!(after.cached_info().state.messages, before.state.messages);
    assert_eq!(
        after.cached_info().state.last_sequence,
        before.state.last_sequence
    );
    provision_event_store(&context, &reduced)
        .await
        .expect("verify updated policy");
    NatsEventStore::connect(context.clone(), reduced)
        .await
        .expect("connect with updated policy");
    context
        .delete_stream(config.stream_name())
        .await
        .expect("cleanup stream");
}

#[tokio::test]
async fn policy_updates_require_an_existing_correctly_scoped_stream() {
    let (context, config) = fixture().await.expect("policy fixture");
    let error = update_event_store(&context, &config)
        .await
        .expect_err("update must not create");
    assert_eq!(error.kind(), EventStoreErrorKind::Unavailable);
    assert!(context.get_stream(config.stream_name()).await.is_err());
    provision_event_store(&context, &config)
        .await
        .expect("create stream");
    let foreign_context = ApplicationName::new("another-application")
        .expect("foreign application")
        .bounded_context("policy")
        .expect("foreign context");
    let foreign =
        NatsEventStoreConfig::new(&foreign_context, config.stream_name()).expect("foreign policy");
    let before = context
        .get_stream(config.stream_name())
        .await
        .expect("existing stream")
        .cached_info()
        .config
        .clone();
    for result in [
        provision_event_store(&context, &foreign).await,
        update_event_store(&context, &foreign).await,
    ] {
        assert_eq!(
            result.expect_err("scope mismatch").kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
    }
    assert_eq!(
        context
            .get_stream(config.stream_name())
            .await
            .expect("preserved scope")
            .cached_info()
            .config,
        before
    );
    context
        .delete_stream(config.stream_name())
        .await
        .expect("cleanup stream");
}

#[tokio::test]
async fn concurrent_provisioners_verify_the_winning_policy() {
    let (context, config) = fixture().await.expect("policy fixture");
    let smaller = config
        .clone()
        .with_storage_limits(16 * 1024 * 1024, config.max_event_bytes())
        .expect("smaller policy");
    let (first, second) = tokio::join!(
        provision_event_store(&context, &config),
        provision_event_store(&context, &smaller)
    );
    let winning_capacity = if first.is_ok() {
        assert_eq!(
            second
                .expect_err("conflicting creator must fail verification")
                .kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
        config.max_stream_bytes()
    } else {
        assert_eq!(
            first
                .expect_err("conflicting creator must fail verification")
                .kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
        second.expect("one creator succeeds");
        smaller.max_stream_bytes()
    };
    assert_eq!(
        context
            .get_stream(config.stream_name())
            .await
            .expect("winning stream")
            .cached_info()
            .config
            .max_bytes,
        winning_capacity
    );
    context
        .delete_stream(config.stream_name())
        .await
        .expect("cleanup stream");

    let (first, second) = tokio::join!(
        provision_event_store(&context, &config),
        provision_event_store(&context, &config)
    );
    first.expect("first identical provisioner");
    second.expect("second identical provisioner");
    context
        .delete_stream(config.stream_name())
        .await
        .expect("cleanup stream");
}

async fn creation_error_proxy(
    context: &jetstream::Context,
    config: &NatsEventStoreConfig,
    winning_capacity: Option<i64>,
) -> TestResult<(jetstream::Context, tokio::task::JoinHandle<TestResult<()>>)> {
    let prefix = format!("{}_RACE_API", config.stream_name());
    let client = context.client().clone();
    let mut requests = client.subscribe(format!("{prefix}.>")).await?;
    client.flush().await?;
    let proxied = jetstream::with_prefix(client.clone(), &prefix);
    let winning_config = config.clone();
    let real = context.clone();
    let proxy = tokio::spawn(async move {
        while let Some(request) = requests.next().await {
            let suffix = request
                .subject
                .strip_prefix(&format!("{prefix}."))
                .ok_or("private API prefix is missing")?;
            let payload = if suffix.starts_with("STREAM.CREATE.") {
                // A concurrent creator can consume the remaining account
                // capacity before NATS checks our duplicate stream name.
                // Create its real stream, then inject that alternate error.
                if let Some(capacity) = winning_capacity {
                    real.create_stream(
                        winning_config
                            .clone()
                            .with_storage_limits(capacity, winning_config.max_event_bytes())?
                            .stream_config(),
                    )
                    .await?;
                }
                serde_json::to_vec(&serde_json::json!({
                    "type": "io.nats.jetstream.api.v1.stream_create_response",
                    "error": {
                        "code": 400,
                        "err_code": 10047,
                        "description": "insufficient storage resources available"
                    }
                }))?
                .into()
            } else {
                if !suffix.starts_with("STREAM.INFO.") {
                    return Err("provisioning must not update policy".into());
                }
                client
                    .request(format!("$JS.API.{suffix}"), request.payload)
                    .await?
                    .payload
            };
            client
                .publish(request.reply.ok_or("API reply is missing")?, payload)
                .await?;
        }
        Ok(())
    });
    Ok((proxied, proxy))
}

#[tokio::test]
async fn creation_errors_reconcile_a_winner_without_updating_its_policy() {
    for winning_capacity in [None, Some(32 * 1024 * 1024), Some(16 * 1024 * 1024)] {
        let (context, config) = fixture().await.expect("race fixture");
        let (proxied, proxy) = creation_error_proxy(&context, &config, winning_capacity)
            .await
            .expect("creation error proxy");
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            provision_event_store(&proxied, &config),
        )
        .await
        .expect("bounded provisioning race");
        proxy.abort();
        match proxy.await {
            Ok(result) => result.expect("API proxy completed successfully"),
            Err(error) => assert!(error.is_cancelled(), "{error}"),
        }
        if let Some(capacity) = winning_capacity {
            let actual = context
                .get_stream(config.stream_name())
                .await
                .expect("winning stream");
            assert_eq!(actual.cached_info().config.max_bytes, capacity);
            context
                .delete_stream(config.stream_name())
                .await
                .expect("cleanup winner");
        }
        match winning_capacity {
            Some(capacity) if capacity == config.max_stream_bytes() => {
                result.expect("identical concurrent creation must verify the winner");
            }
            Some(_) => assert_eq!(
                result.expect_err("different winning policy").kind(),
                EventStoreErrorKind::ConfigurationMismatch
            ),
            None => {
                let error = result.expect_err("actual capacity failure");
                assert_eq!(error.kind(), EventStoreErrorKind::Unavailable);
                assert!(error.message().contains("insufficient storage resources"));
            }
        }
    }
}
