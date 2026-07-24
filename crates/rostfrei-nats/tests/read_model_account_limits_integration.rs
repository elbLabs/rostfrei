//! Account-quota errors from an isolated real NATS server. Requires Docker.

#![allow(clippy::panic_in_result_fn)]

use std::{error::Error as _, num::NonZeroU32, process::Command, time::Duration};

use async_nats::jetstream::{self, stream::StorageType};
use rostfrei::{
    ApplicationName, JsonReadModelCodec, ReadModelErrorKind, ReadModelKey, ReadModelStore,
};
use rostfrei_nats::{
    NatsConnectionConfig, NatsReadModelConfig, NatsReadModelStore, StreamStorage, connect,
    provision_read_model,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn docker(args: &[&str]) -> TestResult<String> {
    let output = Command::new("docker").args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

struct Server(String);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = docker(&["rm", "--force", "--volumes", &self.0]);
    }
}

#[tokio::test]
async fn account_quota_is_capacity_exhausted_for_file_and_memory_buckets() -> TestResult {
    let server = Server(format!("rostfrei-kv-account-quota-{}", std::process::id()));
    let mount = format!(
        "type=bind,src={}/tests/fixtures/read_model_account_limits.conf,dst=/etc/nats/test.conf,readonly",
        env!("CARGO_MANIFEST_DIR")
    );
    docker(&[
        "run",
        "--detach",
        "--name",
        &server.0,
        "--publish",
        "127.0.0.1::4222",
        "--mount",
        &mount,
        "nats:2.12.1-alpine@sha256:b3f2bd84176ae7bd0afa9c48a00f06d7d0818ff4aaee898e4172e0b8340e5816",
        "--config",
        "/etc/nats/test.conf",
    ])?;
    let address = docker(&["port", &server.0, "4222/tcp"])?;
    let config = NatsConnectionConfig::new("kv-account-quota", format!("nats://{address}"))
        .with_user_and_password("quota-test", "fixture-password")
        .with_connection_timeout(Duration::from_millis(500));
    let connection = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(connection) = connect(&config).await
                && connection.jetstream().query_account().await.is_ok()
            {
                break connection;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    for (storage, raw_storage) in [
        (StreamStorage::File, StorageType::File),
        (StreamStorage::Memory, StorageType::Memory),
    ] {
        check_account_quota(connection.jetstream(), storage, raw_storage).await?;
    }
    Ok(())
}

async fn fill_account(context: &jetstream::Context, storage: StorageType) -> TestResult {
    context
        .create_stream(jetstream::stream::Config {
            name: "FILLER".to_owned(),
            subjects: vec!["filler".to_owned()],
            storage,
            ..Default::default()
        })
        .await?;
    for _ in 0..32 {
        if let Err(error) = context
            .publish("filler", vec![b'x'; 4096].into())
            .await?
            .await
        {
            // Prove this is the account quota, not the read-model bucket limit
            // or global server capacity (both server limits are 8 MiB).
            let code = error
                .source()
                .and_then(|source| source.downcast_ref::<jetstream::Error>())
                .map(jetstream::Error::error_code);
            assert_eq!(code, Some(jetstream::ErrorCode::ACCOUNT_RESOURCES_EXCEEDED));
            return Ok(());
        }
    }
    Err("account quota was not reached".into())
}

async fn check_account_quota(
    context: &jetstream::Context,
    storage: StreamStorage,
    raw_storage: StorageType,
) -> TestResult {
    let context_name = ApplicationName::new("quota-test")?.bounded_context("access")?;
    let config = NatsReadModelConfig::new(&context_name, "entitlements")?
        .with_storage(storage)
        .with_storage_limits(16 * 1024, 2048)?;
    provision_read_model(context, &config).await?;
    let store = NatsReadModelStore::connect(
        context.clone(),
        config.clone(),
        JsonReadModelCodec::<String>::new(NonZeroU32::MIN),
    )
    .await?;
    let existing = ReadModelKey::new("existing")?;
    let original = "original".to_owned();
    let revision = store.create(&existing, &original).await?;
    fill_account(context, raw_storage).await?;

    // Smaller writes may consume the space left after the 4 KiB filler fails.
    // The bucket remains below its own 16 KiB limit when the account rejects us.
    let value = "x".repeat(1900);
    let mut rejected = None;
    for index in 0..8 {
        let key = ReadModelKey::new(format!("new-{index}"))?;
        if let Err(error) = store.create(&key, &value).await {
            assert_eq!(error.kind(), ReadModelErrorKind::CapacityExhausted);
            rejected = Some(key);
            break;
        }
    }
    let rejected = rejected.ok_or("account quota did not reject read-model creation")?;
    let info = context
        .get_stream(config.stream_name())
        .await?
        .cached_info()
        .clone();
    assert!(info.state.bytes < u64::try_from(config.max_bytes())?);
    assert!(store.read(&rejected).await?.is_none());
    let Err(error) = store.update(&existing, &revision, &value).await else {
        return Err("account quota did not reject read-model update".into());
    };
    assert_eq!(error.kind(), ReadModelErrorKind::CapacityExhausted);
    assert_eq!(
        store
            .read(&existing)
            .await?
            .ok_or("missing original")?
            .value,
        original
    );

    context.delete_stream("FILLER").await?;
    store.create(&rejected, &value).await?;
    store.update(&existing, &revision, &value).await?;
    context.delete_stream(config.stream_name()).await?;
    Ok(())
}
