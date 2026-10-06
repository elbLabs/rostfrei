//! Isolated, authenticated broker restart acceptance test. Requires Docker.

#![allow(clippy::panic_in_result_fn)]

use std::{net::TcpListener, num::NonZeroU32, process::Command, time::Duration};

use rostfrei::{
    ApplicationName, JsonReadModelCodec, ReadModelErrorKind, ReadModelKey, ReadModelStore,
};
use rostfrei_nats::{
    ConnectionHealth, NatsConnectionConfig, NatsReadModelConfig, NatsReadModelStore, connect,
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
        let _ = docker(&["rm", "--force", &self.0]);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn snapshots_tombstones_and_cas_survive_authenticated_broker_restart() -> TestResult {
    let server = Server(format!("rostfrei-kv-restart-{}", std::process::id()));
    // Docker may reassign an automatically published port on restart. Reserve
    // a concrete loopback port so reconnect exercises the same server address.
    let address = TcpListener::bind("127.0.0.1:0")?.local_addr()?.to_string();
    docker(&[
        "run",
        "--detach",
        "--name",
        &server.0,
        "--publish",
        &format!("{address}:4222"),
        "nats:2.12.1-alpine",
        "--jetstream",
        "--store_dir",
        "/data",
        "--user",
        "reader",
        "--pass",
        "test-password",
    ])?;
    let config = NatsConnectionConfig::new("kv-restart", format!("nats://{address}"))
        .with_user_and_password("reader", "test-password")
        .with_connection_timeout(Duration::from_millis(500));
    let connection = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Ok(connection) = connect(&config).await {
                break connection;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "broker startup timed out")?;
    let context = ApplicationName::new("restart-test")?.bounded_context("access")?;
    let config = NatsReadModelConfig::new(&context, "entitlements")?;
    provision_read_model(connection.jetstream(), &config).await?;
    let store = NatsReadModelStore::connect(
        connection.jetstream().clone(),
        config.clone(),
        JsonReadModelCodec::<String>::new(NonZeroU32::MIN),
    )
    .await?;
    let key = ReadModelKey::new("live")?;
    let deleted = ReadModelKey::new("deleted")?;
    let value = "persisted".to_owned();
    let revision = store.create(&key, &value).await?;
    let stale = store.create(&deleted, &value).await?;
    store.delete(&deleted, &stale).await?;

    docker(&["stop", "--time", "5", &server.0])?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while connection.health() == ConnectionHealth::Connected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    assert_eq!(
        store.read(&key).await.unwrap_err().kind(),
        ReadModelErrorKind::Unavailable
    );
    assert_eq!(
        store
            .update(&key, &revision, &value)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Unavailable
    );

    docker(&["start", &server.0])?;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if connection.check_health().await.is_ok() && store.read(&key).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    // The stopped-broker update may have been buffered and committed after
    // reconnect: an Unavailable result is deliberately NOT a no-write promise.
    let persisted = store
        .read(&key)
        .await?
        .ok_or("missing persisted snapshot")?;
    assert_eq!(persisted.value, value);
    store
        .update(&key, &persisted.revision, &"after restart".to_owned())
        .await?;
    assert!(store.read(&deleted).await?.is_none());
    assert_ne!(store.create(&deleted, &value).await?, stale);
    assert_eq!(
        store
            .update(&deleted, &stale, &value)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::Conflict
    );

    // Recreating a bucket starts a new revision incarnation. A new handle
    // rejects the old token even if the underlying sequence number repeats.
    connection
        .jetstream()
        .delete_stream(config.stream_name())
        .await?;
    provision_read_model(connection.jetstream(), &config).await?;
    let fresh = NatsReadModelStore::connect(
        connection.jetstream().clone(),
        config,
        JsonReadModelCodec::<String>::new(NonZeroU32::MIN),
    )
    .await?;
    fresh.create(&key, &value).await?;
    assert_eq!(
        fresh
            .update(&key, &revision, &value)
            .await
            .unwrap_err()
            .kind(),
        ReadModelErrorKind::InvalidRequest
    );
    Ok(())
}
