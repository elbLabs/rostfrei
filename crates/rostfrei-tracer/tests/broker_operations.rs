#![cfg(feature = "nats-host")]
#![allow(clippy::expect_used)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_nats::jetstream::{self, consumer, kv, stream};
use futures_util::StreamExt;
use rostfrei_nats::{NatsConnectionConfig, connect};
use rostfrei_tracer::broker_operations::{
    BrokerConnectionStatus, BrokerConsumerLiveness, BrokerOperations, BrokerReadStatus,
    NatsBrokerOperations, NatsBrokerSource,
};

fn broker_config() -> NatsConnectionConfig {
    let url = std::env::var("ROSTFREI_INVENTORY_NATS_URL")
        .expect("use cargo xtask test to supply a dedicated disposable inventory account");
    NatsConnectionConfig::new("broker-operations-contract", url)
}

fn stream_name() -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    format!("ops_{nonce}")
}

async fn seed_account_resources(js: &jetstream::Context, name: &str, bucket: &str) {
    let subject = format!("privateSubject.{name}");
    let configured_stream = js
        .create_stream(stream::Config {
            name: name.to_owned(),
            subjects: vec![subject.clone()],
            description: Some("private-description".into()),
            storage: stream::StorageType::Memory,
            max_bytes: 4096,
            ..Default::default()
        })
        .await
        .expect("create stream");
    let worker: consumer::PullConsumer = configured_stream
        .create_consumer(consumer::pull::Config {
            durable_name: Some("worker".into()),
            ..Default::default()
        })
        .await
        .expect("create consumer");
    for _ in 0..2 {
        js.publish(subject.clone(), "private-payload".into())
            .await
            .expect("publish")
            .await
            .expect("persist message");
    }
    let mut batch = worker
        .fetch()
        .max_messages(1)
        .messages()
        .await
        .expect("fetch");
    let _unacked = tokio::time::timeout(Duration::from_secs(3), batch.next())
        .await
        .expect("delivery timed out")
        .expect("delivery missing")
        .expect("message");
    let kv = js
        .create_key_value(kv::Config {
            bucket: bucket.to_owned(),
            max_bytes: 8192,
            ..Default::default()
        })
        .await
        .expect("create KV");
    kv.put("private-key", "private-value".into())
        .await
        .expect("persist KV value");
}

#[tokio::test]
async fn samples_account_resources_even_when_another_source_is_unavailable() {
    let config = broker_config();
    let connection = connect(&config)
        .await
        .expect("connect to disposable broker");
    let js = connection.jetstream();
    let name = stream_name();
    let bucket = format!("{name}_kv");
    seed_account_resources(js, &name, &bucket).await;
    let missing = NatsConnectionConfig::new("private-client-name", "invalid://127.0.0.1:1");
    let inspector = NatsBrokerOperations::new([
        NatsBrokerSource::configured("unavailable account", missing).expect("source label"),
        NatsBrokerSource::configured("disposable account", config).expect("source label"),
    ])
    .expect("configured operations sources");
    let snapshot = inspector.snapshot().await;
    let missing = snapshot.sources.first().expect("missing source retained");
    assert_eq!(missing.label, "unavailable account");
    assert_eq!(missing.connection, BrokerConnectionStatus::Unavailable);
    assert!(missing.account.usage.is_none());
    assert_eq!(missing.streams.status, BrokerReadStatus::Unavailable);

    let source = snapshot.sources.get(1).expect("healthy source retained");
    assert_eq!(source.connection, BrokerConnectionStatus::Connected);
    let account = source.account.usage.as_ref().expect("account usage");
    assert!(account.memory.used_bytes > 0);
    assert!(account.storage.used_bytes > 0);
    assert_eq!(source.streams.status, BrokerReadStatus::Complete);
    assert_eq!(source.kv_buckets.status, BrokerReadStatus::Complete);
    let row = source
        .streams
        .items
        .iter()
        .find(|row| row.name == name)
        .expect("stream");
    assert_eq!(row.messages, 2);
    assert_eq!(row.capacity.limit_bytes, Some(4096));
    assert_eq!(row.consumers.status, BrokerReadStatus::Complete);
    let consumer = row.consumers.items.first().expect("consumer");
    assert_eq!(consumer.name, "worker");
    assert_eq!(consumer.pending, 1);
    assert_eq!(consumer.ack_pending, 1);
    assert_eq!(consumer.delivered_stream_sequence, "1");
    assert_eq!(consumer.ack_floor_stream_sequence, "0");
    assert_eq!(consumer.liveness, BrokerConsumerLiveness::Unknown);
    let row = source
        .kv_buckets
        .items
        .iter()
        .find(|row| row.name == bucket)
        .expect("KV bucket");
    assert_eq!(row.capacity.limit_bytes, Some(8192));
    assert!(row.capacity.used_bytes > 0);
    let json = serde_json::to_string(&snapshot).expect("wire snapshot");
    for secret in [
        "private-client-name",
        "privateSubject",
        "private-description",
        "private-payload",
        "private-key",
        "private-value",
    ] {
        assert!(!json.contains(secret), "disclosed {secret}");
    }

    js.delete_stream(name).await.expect("remove test stream");
    js.delete_key_value(bucket)
        .await
        .expect("remove test KV bucket");
}

#[tokio::test]
async fn reports_consumer_inventory_as_truncated_without_hiding_the_stream() {
    let connection = connect(&broker_config())
        .await
        .expect("connect to disposable broker");
    let js = connection.jetstream();
    let name = stream_name();
    let configured_stream = js
        .create_stream(stream::Config {
            name: name.clone(),
            subjects: vec![format!("test.{name}")],
            storage: stream::StorageType::Memory,
            ..Default::default()
        })
        .await
        .expect("create stream");
    for index in 0..51 {
        configured_stream
            .create_consumer(consumer::pull::Config {
                durable_name: Some(format!("worker_{index:02}")),
                ..Default::default()
            })
            .await
            .expect("create consumer");
    }
    let reader = NatsBrokerOperations::new([NatsBrokerSource::connected(
        "disposable account",
        connection.clone(),
    )
    .expect("source label")])
    .expect("configured operations source");
    let snapshot = reader.snapshot().await;
    let source = snapshot.sources.first().expect("source retained");
    assert_eq!(source.streams.status, BrokerReadStatus::Complete);
    let row = source
        .streams
        .items
        .iter()
        .find(|row| row.name == name)
        .expect("stream retained");
    assert_eq!(row.consumer_count, 51);
    assert_eq!(row.consumers.items.len(), 50);
    assert_eq!(row.consumers.status, BrokerReadStatus::Truncated);
    js.delete_stream(name).await.expect("remove test stream");
}
