use base64::{Engine as _, engine::general_purpose::STANDARD};
use rostfrei_messaging_core::{
    ApplicationName, QuarantineDiagnostic, QuarantineFilter, QuarantineMessageKind,
    QuarantineQuery, QuarantineReadError, QuarantineReader, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, MessagingTopology, NatsConnection, NatsConnectionConfig,
    NatsQuarantineReader, connect, provision_application_messaging,
};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    connection: NatsConnection,
    test: ApplicationMessagingConfig,
    production: ApplicationMessagingConfig,
}

// These helpers construct disposable test fixtures; a failed setup should fail the test.
#[allow(clippy::unwrap_used)]
impl Fixture {
    async fn new() -> Option<Self> {
        let url = std::env::var("ROSTFREI_NATS_URL").ok()?;
        let application = ApplicationName::new(format!(
            "quarantine-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
        .unwrap();
        let connection = connect(&NatsConnectionConfig::new("quarantine-tests", url))
            .await
            .unwrap();
        let test = ApplicationMessagingConfig::new_in_scope(&application, TrafficScope::Test)
            .unwrap()
            .with_max_bytes(64 * 1024 * 1024)
            .unwrap();
        let production = ApplicationMessagingConfig::new(&application)
            .unwrap()
            .with_max_bytes(64 * 1024 * 1024)
            .unwrap();
        for config in [&test, &production] {
            provision_application_messaging(connection.jetstream(), config)
                .await
                .unwrap();
        }
        Some(Self {
            connection,
            test,
            production,
        })
    }

    fn reader(&self, topology: &MessagingTopology) -> NatsQuarantineReader {
        NatsQuarantineReader::new(self.connection.jetstream().clone(), topology.clone())
    }

    fn subject(&self, scope: TrafficScope, kind: &str, name: &str) -> String {
        let application = self.test.application().as_str();
        let scope = if scope == TrafficScope::Test {
            ".test"
        } else {
            ""
        };
        format!("{application}{scope}.quarantine.{kind}.orders.{name}")
    }

    async fn publish(&self, scope: TrafficScope, name: &str, payload: Vec<u8>) {
        self.connection
            .jetstream()
            .publish(self.subject(scope, "command", name), payload.into())
            .await
            .unwrap()
            .await
            .unwrap();
    }

    fn record(&self, payload: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "message_id": "original-message",
            "address": format!("{}.test.command.orders.place", self.test.application()),
            "payload_base64": STANDARD.encode(payload),
            "metadata": {"x-customer": "customer-1"},
            "trace_context": null,
            "reason": "test failure",
            "attempt": 3,
            "pending": 1,
            "source_sequence": 9_007_199_254_740_993_u64,
            "consumer_sequence": 7,
            "source_stream": self.test.topology().command_stream().as_str(),
            "source_consumer": "orders-worker"
        }))
        .unwrap()
    }

    async fn cleanup(&self) {
        for config in [&self.test, &self.production] {
            for stream in config.streams() {
                self.connection
                    .delete_stream_if_exists(stream.name().as_str())
                    .await
                    .unwrap();
            }
        }
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn snapshot_reads_handle_gaps_malformed_records_and_scope_isolation() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    fixture
        .publish(
            TrafficScope::Test,
            "place",
            fixture.record(br#"{"secret":"retained"}"#),
        )
        .await;
    fixture
        .publish(TrafficScope::Test, "place", fixture.record(b"second"))
        .await;
    fixture
        .publish(
            TrafficScope::Test,
            "place",
            b"not a quarantine record".to_vec(),
        )
        .await;
    let reader = fixture.reader(fixture.test.topology());
    let all = reader.list(QuarantineQuery::default()).await.unwrap();
    let first = reader
        .list(QuarantineQuery {
            limit: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first.snapshot_sequence, 3);
    assert_eq!(first.retained_messages, 3);
    let original = reader.get(&first.items[0].id).await.unwrap();
    assert_eq!(original, first.items[0]);
    assert_eq!(
        original.message.as_ref().unwrap().source_sequence,
        9_007_199_254_740_993
    );
    assert!(original.diagnostics.is_empty());

    // A deletion creates a gap; a later append is outside the original traversal.
    let stream = fixture
        .connection
        .jetstream()
        .get_stream(fixture.test.topology().quarantine_stream().as_str())
        .await
        .unwrap();
    stream.delete_message(2).await.unwrap();
    fixture
        .publish(TrafficScope::Test, "place", fixture.record(b"late"))
        .await;
    let page = reader
        .list(QuarantineQuery {
            cursor: first.next_cursor.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(page.next_cursor.is_none());
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].diagnostics,
        [QuarantineDiagnostic::InvalidRecord]
    );
    assert_eq!(
        STANDARD
            .decode(page.items[0].invalid_record_base64.as_ref().unwrap())
            .unwrap(),
        b"not a quarantine record"
    );
    assert_eq!(
        reader.get(&all.items[1].id).await,
        Err(QuarantineReadError::NotFound)
    );
    assert_eq!(
        fixture
            .reader(fixture.production.topology())
            .get(&original.id)
            .await,
        Err(QuarantineReadError::StaleGeneration)
    );
    assert!(
        fixture
            .reader(fixture.production.topology())
            .list(QuarantineQuery::default())
            .await
            .unwrap()
            .items
            .is_empty()
    );

    assert_eq!(
        reader
            .list(QuarantineQuery {
                cursor: first.next_cursor,
                filter: QuarantineFilter {
                    name: Some("different".to_owned()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await,
        Err(QuarantineReadError::InvalidCursor)
    );
    let info = fixture
        .connection
        .jetstream()
        .get_stream(fixture.test.topology().quarantine_stream().as_str())
        .await
        .unwrap();
    assert_eq!(info.cached_info().state.messages, 3);
    assert_eq!(info.cached_info().state.consumer_count, 0);
    fixture.cleanup().await;
}

#[tokio::test]
async fn reset_invalidates_record_links_and_cursors_even_when_sequences_are_reused() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    for _ in 0..2 {
        fixture
            .publish(TrafficScope::Test, "place", fixture.record(b"old"))
            .await;
    }
    let reader = fixture.reader(fixture.test.topology());
    let page = reader
        .list(QuarantineQuery {
            limit: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    fixture
        .connection
        .delete_stream_if_exists(fixture.test.topology().quarantine_stream().as_str())
        .await
        .unwrap();
    assert_eq!(
        reader.get(&page.items[0].id).await,
        Err(QuarantineReadError::StaleGeneration)
    );
    provision_application_messaging(fixture.connection.jetstream(), &fixture.test)
        .await
        .unwrap();
    fixture
        .publish(TrafficScope::Test, "place", fixture.record(b"new"))
        .await;
    assert_eq!(
        reader.get(&page.items[0].id).await,
        Err(QuarantineReadError::StaleGeneration)
    );
    assert_eq!(
        reader
            .list(QuarantineQuery {
                cursor: page.next_cursor,
                ..Default::default()
            })
            .await,
        Err(QuarantineReadError::StaleGeneration)
    );
    assert_ne!(
        reader.list(QuarantineQuery::default()).await.unwrap().items[0].id,
        page.items[0].id
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn pages_are_byte_bounded_and_filters_are_applied_by_subject() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let payload = fixture.record(&vec![b'x'; 1024 * 1024]);
    for _ in 0..9 {
        fixture
            .publish(TrafficScope::Test, "place", payload.clone())
            .await;
    }
    let reader = fixture.reader(fixture.test.topology());
    let first = reader
        .list(QuarantineQuery {
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(first.items.len() < 9);
    assert!(first.next_cursor.is_some());
    let rest = reader
        .list(QuarantineQuery {
            cursor: first.next_cursor,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first.items.len().saturating_add(rest.items.len()), 9);
    let filtered = reader
        .list(QuarantineQuery {
            filter: QuarantineFilter {
                kind: Some(QuarantineMessageKind::IntegrationEvent),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(filtered.items.is_empty());
    assert!(filtered.next_cursor.is_none());
    fixture.cleanup().await;
}

#[tokio::test]
async fn invalid_payloads_remain_inspectable_and_wrong_topology_is_rejected() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let mut record: serde_json::Value = serde_json::from_slice(&fixture.record(b"prefix")).unwrap();
    record["payload_truncated"] = json!(true);
    record["payload_size"] = json!(123_456);
    record["payload_base64"] = json!("not-base64!");
    fixture
        .publish(
            TrafficScope::Test,
            "place",
            serde_json::to_vec(&record).unwrap(),
        )
        .await;
    let page = fixture
        .reader(fixture.test.topology())
        .list(QuarantineQuery::default())
        .await
        .unwrap();
    assert_eq!(
        page.items[0].diagnostics,
        [
            QuarantineDiagnostic::InvalidPayloadEncoding,
            QuarantineDiagnostic::PayloadTruncated
        ]
    );
    let topology = MessagingTopology::new_in_scope(
        fixture.test.application().clone(),
        TrafficScope::Test,
        fixture.test.topology().command_stream().clone(),
        fixture.test.topology().integration_event_stream().clone(),
        fixture.production.topology().quarantine_stream().clone(),
    )
    .unwrap();
    assert_eq!(
        fixture
            .reader(&topology)
            .list(QuarantineQuery::default())
            .await,
        Err(QuarantineReadError::InvalidTopology)
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn inspection_includes_unknown_subjects_and_preserves_consumer_progress() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    fixture
        .publish(TrafficScope::Test, "place", fixture.record(b"payload"))
        .await;
    fixture
        .connection
        .jetstream()
        .publish(
            format!("{}.test.quarantine.unknown", fixture.test.application()),
            "malformed".into(),
        )
        .await
        .unwrap()
        .await
        .unwrap();
    let stream = fixture
        .connection
        .jetstream()
        .get_stream(fixture.test.topology().quarantine_stream().as_str())
        .await
        .unwrap();
    let mut consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("inspection-progress".to_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    let before = consumer.info().await.unwrap().clone();
    let reader = fixture.reader(fixture.test.topology());
    let page = reader.list(QuarantineQuery::default()).await.unwrap();
    assert_eq!(page.items.len(), 2);
    for entry in page.items {
        assert_eq!(reader.get(&entry.id).await.unwrap(), entry);
    }
    let after = consumer.info().await.unwrap();
    assert_eq!(after.delivered, before.delivered);
    assert_eq!(after.ack_floor, before.ack_floor);
    assert_eq!(after.num_pending, before.num_pending);
    assert_eq!(after.num_ack_pending, before.num_ack_pending);
    fixture.cleanup().await;
}
