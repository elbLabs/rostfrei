use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream;
use futures_util::StreamExt as _;
use rostfrei_core::EventStoreErrorKind;
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsEventStore, NatsEventStoreConfig, provision_event_store, update_event_store,
};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

async fn fixture() -> TestResult<(jetstream::Context, NatsEventStoreConfig)> {
    let client = async_nats::connect(rostfrei_testing::integration::nats_url()?).await?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let context = ApplicationName::new(format!("policy-test-{nanos}-{sequence}"))?
        .bounded_context("policy")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(32 * 1024 * 1024, 512 * 1024)?;
    Ok((jetstream::new(client), config))
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
