#![allow(clippy::panic_in_result_fn)]

use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use async_nats::jetstream;
use rostfrei::{
    ApplicationName, JsonReadModelCodec, ReadModelCodec, ReadModelErrorKind, ReadModelKey,
    ReadModelStore, TrafficScope,
};
use rostfrei_nats::{
    NatsConnectionConfig, NatsReadModelConfig, NatsReadModelStore, connect, provision_read_model,
    update_read_model, verify_read_model,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type Store = NatsReadModelStore<String, JsonReadModelCodec<String>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

async fn fixture() -> TestResult<(jetstream::Context, NatsReadModelConfig)> {
    let connection = connect(&NatsConnectionConfig::new(
        "read-model-tests",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?;
    let application = ApplicationName::new(format!(
        "read-model-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))?;
    let config = NatsReadModelConfig::new(&application.bounded_context("access")?, "entitlement")?;
    Ok((connection.jetstream().clone(), config))
}

async fn open(context: &jetstream::Context, config: &NatsReadModelConfig) -> TestResult<Store> {
    Ok(Store::connect(
        context.clone(),
        config.clone(),
        JsonReadModelCodec::new(NonZeroU32::MIN),
    )
    .await?)
}

#[tokio::test]
async fn cas_concurrency_tombstones_and_recreation() -> TestResult {
    let (context, config) = fixture().await?;
    provision_read_model(&context, &config).await?;
    let store = open(&context, &config).await?;
    let key = ReadModelKey::new("organization-1")?;
    assert!(store.read(&key).await?.is_none());
    let initial = "initial".to_owned();
    let (left, right) = tokio::join!(store.create(&key, &initial), store.create(&key, &initial));
    assert_ne!(left.is_ok(), right.is_ok());
    let revision = match (left, right) {
        (Ok(revision), Err(error)) | (Err(error), Ok(revision)) => {
            assert_eq!(error.kind(), ReadModelErrorKind::Conflict);
            revision
        }
        _ => return Err("expected exactly one creator".into()),
    };
    assert_eq!(store.read(&key).await?.ok_or("missing")?.revision, revision);
    let left_value = "left".to_owned();
    let right_value = "right".to_owned();
    let (left, right) = tokio::join!(
        store.update(&key, &revision, &left_value),
        store.update(&key, &revision, &right_value)
    );
    let winner = match (left, right) {
        (Ok(revision), Err(error)) | (Err(error), Ok(revision)) => {
            assert_eq!(error.kind(), ReadModelErrorKind::Conflict);
            revision
        }
        _ => return Err("expected exactly one updater".into()),
    };
    assert_eq!(
        store.delete(&key, &revision).await.unwrap_err().kind(),
        ReadModelErrorKind::Conflict
    );
    store.delete(&key, &winner).await?;
    assert!(store.read(&key).await?.is_none());
    assert_eq!(
        store.delete(&key, &winner).await.unwrap_err().kind(),
        ReadModelErrorKind::Conflict
    );
    assert_eq!(
        store
            .update(&key, &winner, &initial)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Conflict
    );
    let recreated = store.create(&key, &initial).await?;
    assert_ne!(recreated, winner);
    assert_eq!(
        store
            .update(&key, &winner, &initial)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Conflict
    );
    let another_key = ReadModelKey::new("organization-2")?;
    assert_eq!(
        store
            .update(&another_key, &recreated, &initial)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::InvalidRequest
    );
    // A freshly connected handle sees the persisted snapshot and accepts its token.
    let reconnected = open(&context, &config).await?;
    assert_eq!(
        reconnected.read(&key).await?.ok_or("missing")?.value,
        initial
    );
    reconnected.update(&key, &recreated, &right_value).await?;
    context.delete_stream(config.stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn policy_is_explicit_verified_and_race_safe() -> TestResult {
    let (context, config) = fixture().await?;
    assert!(open(&context, &config).await.is_err());
    assert!(context.get_stream(config.stream_name()).await.is_err());
    assert_eq!(
        update_read_model(&context, &config)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Unavailable
    );
    let changed = config
        .clone()
        .with_history(2)?
        .with_storage_limits(32 * 1024 * 1024, 256 * 1024)?;
    let (one, two) = tokio::join!(
        provision_read_model(&context, &config),
        provision_read_model(&context, &changed)
    );
    let winner = match (one, two) {
        (Ok(()), Err(error)) => {
            assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
            &config
        }
        (Err(error), Ok(())) => {
            assert_eq!(error.kind(), ReadModelErrorKind::ConfigurationMismatch);
            &changed
        }
        _ => return Err("exactly one provisioning policy must win".into()),
    };
    let store = open(&context, winner).await?;
    let key = ReadModelKey::new("one")?;
    store.create(&key, &"preserved".to_owned()).await?;
    let updated = winner.clone().with_history(3)?;
    assert_eq!(
        provision_read_model(&context, &updated)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::ConfigurationMismatch
    );
    verify_read_model(&context, winner).await?;
    update_read_model(&context, &updated).await?;
    assert_eq!(
        open(&context, &updated)
            .await?
            .read(&key)
            .await?
            .ok_or("missing")?
            .value,
        "preserved"
    );
    // Same name but foreign routing cannot be repaired implicitly, even by update.
    let mut foreign = context
        .get_stream(config.stream_name())
        .await?
        .cached_info()
        .config
        .clone();
    foreign.subjects = vec!["foreign.read-model.>".to_owned()];
    context.update_stream(foreign).await?;
    assert_eq!(
        verify_read_model(&context, &updated)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::ConfigurationMismatch
    );
    assert_eq!(
        update_read_model(&context, &updated)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::ConfigurationMismatch
    );
    context.delete_stream(config.stream_name()).await?;
    Ok(())
}

#[tokio::test]
async fn resources_are_isolated_by_every_scope_dimension() -> TestResult {
    let (context, config) = fixture().await?;
    let application = config.context().application();
    let configs = [
        config.clone(),
        NatsReadModelConfig::new(
            &application.bounded_context_in_scope(TrafficScope::Test, "access")?,
            "entitlement",
        )?,
        NatsReadModelConfig::new(&application.bounded_context("membership")?, "entitlement")?,
        NatsReadModelConfig::new(config.context(), "summary")?,
        NatsReadModelConfig::new(
            &ApplicationName::new(format!("{}-other", application.as_str()))?
                .bounded_context("access")?,
            "entitlement",
        )?,
    ];
    let key = ReadModelKey::new("same-key")?;
    for config in &configs {
        provision_read_model(&context, config).await?;
        open(&context, config)
            .await?
            .create(&key, &config.bucket_name().to_owned())
            .await?;
    }
    for config in &configs {
        assert_eq!(
            open(&context, config)
                .await?
                .read(&key)
                .await?
                .ok_or("missing")?
                .value,
            config.bucket_name()
        );
        context.delete_stream(config.stream_name()).await?;
    }
    Ok(())
}

#[tokio::test]
async fn validates_payload_schema_capacity_and_unavailability() -> TestResult {
    let (context, config) = fixture().await?;
    let config = config.with_storage_limits(4096, 256)?;
    provision_read_model(&context, &config).await?;
    let store = open(&context, &config).await?;
    let key = ReadModelKey::new("one")?;
    assert_eq!(
        store
            .create(&key, &"x".repeat(256))
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::PayloadTooLarge
    );
    let raw = context.get_key_value(config.bucket_name()).await?;
    for (bytes, expected) in [
        (br"not-json".to_vec(), ReadModelErrorKind::InvalidData),
        (
            br#"{"schema_version":1,"value":42}"#.to_vec(),
            ReadModelErrorKind::InvalidData,
        ),
        (
            br#"{"schema_version":2,"value":{}}"#.to_vec(),
            ReadModelErrorKind::IncompatibleSchema,
        ),
        (vec![b'x'; 257], ReadModelErrorKind::PayloadTooLarge),
    ] {
        raw.put(key.as_str(), bytes.into()).await?;
        assert_eq!(store.read(&key).await.unwrap_err().kind(), expected);
    }
    let mut capacity_error = None;
    for index in 0..100 {
        let key = ReadModelKey::new(format!("capacity-{index}"))?;
        if let Err(error) = store.create(&key, &"x".repeat(200)).await {
            capacity_error = Some(error);
            break;
        }
    }
    assert_eq!(
        capacity_error.ok_or("bucket was not bounded")?.kind(),
        ReadModelErrorKind::CapacityExhausted
    );
    context.delete_stream(config.stream_name()).await?;
    assert_eq!(
        store.read(&key).await.unwrap_err().kind(),
        ReadModelErrorKind::Unavailable
    );
    assert_eq!(
        store
            .create(&key, &"value".to_owned())
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Unavailable
    );
    Ok(())
}

#[tokio::test]
async fn ttl_expires_values_and_allows_recreation() -> TestResult {
    let (context, config) = fixture().await?;
    let config = config.with_ttl(Some(Duration::from_millis(200)))?;
    provision_read_model(&context, &config).await?;
    let store = open(&context, &config).await?;
    let key = ReadModelKey::new("expires")?;
    let value = "value".to_owned();
    let revision = store.create(&key, &value).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while store.read(&key).await?.is_some() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    assert_eq!(
        store
            .update(&key, &revision, &value)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Conflict
    );
    assert_ne!(store.create(&key, &value).await?, revision);
    context.delete_stream(config.stream_name()).await?;
    Ok(())
}

#[test]
fn rejects_invalid_policy_and_names() -> TestResult {
    let context = ApplicationName::new("acme")?.bounded_context("access")?;
    assert!(NatsReadModelConfig::new(&context, "bad.name").is_err());
    let config = NatsReadModelConfig::new(&context, "entitlements")?;
    assert_eq!(
        config.bucket_name(),
        "ACME__ACCESS__ENTITLEMENTS_READ_MODEL"
    );
    assert!(config.clone().with_history(65).is_err());
    assert!(config.clone().with_history(0).is_err());
    assert!(config.clone().with_replicas(0).is_err());
    assert!(config.clone().with_storage_limits(1, 1).is_err());
    assert!(
        config
            .clone()
            .with_storage_limits(i64::MAX, i32::MAX)
            .is_err()
    );
    assert!(config.with_ttl(Some(Duration::ZERO)).is_err());
    // Codec failures remain distinguishable even before touching NATS.
    let codec = JsonReadModelCodec::<String>::new(NonZeroU32::MIN);
    assert_eq!(
        codec
            .decode(br#"{"schema_version":0,"value":"x"}"#)
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::IncompatibleSchema
    );
    Ok(())
}
