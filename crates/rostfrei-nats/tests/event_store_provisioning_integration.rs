use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream;
use rostfrei_core::EventStoreErrorKind;
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsEventStore, NatsEventStoreConfig, provision_event_store, update_event_store,
};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

async fn fixture() -> TestResult<(jetstream::Context, NatsEventStoreConfig)> {
    let client = async_nats::connect(std::env::var("ROSTFREI_NATS_URL")?).await?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let context = ApplicationName::new(format!("policy-test-{nanos}-{sequence}"))?
        .bounded_context("policy")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(32 * 1024 * 1024, 512 * 1024)?;
    Ok((jetstream::new(client), config))
}

#[tokio::test]
#[ignore = "requires a real NATS server configured by ROSTFREI_NATS_URL"]
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
#[ignore = "requires a real NATS server configured by ROSTFREI_NATS_URL"]
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
#[ignore = "requires a real NATS server configured by ROSTFREI_NATS_URL"]
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
