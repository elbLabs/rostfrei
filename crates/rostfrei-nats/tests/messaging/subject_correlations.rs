#![allow(clippy::panic_in_result_fn)]

use super::*;
use async_nats::jetstream::stream::RawMessageErrorKind;
use rostfrei_messaging_core::{
    CausationId, IntegrationEventAddress, IntegrationEventEnvelope, MAX_IDENTIFIER_BYTES,
    QuarantineFailureKind,
};

#[path = "integration_transport.rs"]
mod integration_transport;

fn event(
    id: &str,
    correlation: &str,
    causation: Option<&str>,
) -> TestResult<IntegrationEventEnvelope<Value>> {
    Ok(IntegrationEventEnvelope::new(
        EnvelopeContext::new(
            MessageId::new(id)?,
            SchemaVersion::new(7)?,
            CorrelationId::new(correlation)?,
            causation.map(CausationId::new).transpose()?,
        ),
        MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?,
        json!({"business_value": 42}),
    )?)
}

fn outbound(
    address: &IntegrationEventAddress,
    envelope: &IntegrationEventEnvelope<Value>,
) -> TestResult<OutboundMessage<IntegrationEventAddress>> {
    Ok(OutboundMessage::json(
        address.clone(),
        envelope.message_id().clone(),
        envelope,
    )?)
}

async fn integration_stream(
    fixture: &Fixture,
) -> TestResult<async_nats::jetstream::stream::Stream> {
    Ok(fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.integration_event_stream().as_str())
        .await?)
}

async fn native_filtered_ids(fixture: &Fixture, filter: &str) -> TestResult<Vec<String>> {
    let stream = integration_stream(fixture).await?;
    let cutoff = stream.cached_info().state.last_sequence;
    let mut next = 1;
    let mut ids = Vec::new();
    while next <= cutoff {
        match stream.get_first_raw_message_by_subject(filter, next).await {
            Ok(message) => {
                next = message.sequence.checked_add(1).ok_or("sequence overflow")?;
                let envelope: IntegrationEventEnvelope<Value> =
                    serde_json::from_slice(&message.payload)?;
                ids.push(envelope.message_id().as_str().to_owned());
            }
            Err(error) if error.kind() == RawMessageErrorKind::NoMessageFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(ids)
}

#[tokio::test]
async fn publication_stores_readable_and_escaped_control_ids_without_changing_envelopes()
-> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let publisher = fixture.connection.publisher(fixture.topology.clone());
    let stream = integration_stream(&fixture).await?;
    let examples = [
        (
            "external-message-1",
            "550e8400-e29b-41d4-a716-446655440001",
            Some("550e8400-e29b-41d4-a716-446655440002"),
            "550e8400-e29b-41d4-a716-446655440001.550e8400-e29b-41d4-a716-446655440002",
        ),
        (
            "external/message:2",
            "Flow_1.*>%/~",
            Some("Cause-2.*>%/~"),
            "Flow_1%2E%2A%3E%25%2F%7E.Cause-2%2E%2A%3E%25%2F%7E",
        ),
        (
            "external-message-3",
            "without-cause",
            None,
            "without-cause.~",
        ),
        ("external-message-4", "~", Some("~"), "%7E.%7E"),
        ("external-message-5", "%7E", Some("%7E"), "%257E.%257E"),
    ];
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (id, correlation, causation, suffix) in examples {
        let envelope = event(id, correlation, causation)?;
        let message =
            outbound(&address, &envelope)?.with_correlation_id(envelope.correlation_id().clone());
        let ack = publisher
            .publish_integration_event_with_ack(message.clone(), Duration::from_secs(5))
            .await?;
        let duplicate = publisher
            .publish_integration_event_with_ack(message.clone(), Duration::from_secs(5))
            .await?;
        assert!(duplicate.duplicate());
        assert_eq!(duplicate.sequence(), ack.sequence());
        let stored = stream.get_raw_message(ack.sequence()).await?;
        assert_eq!(stored.payload.as_ref(), message.payload());
        assert_eq!(
            stored
                .headers
                .get("Nats-Msg-Id")
                .map(async_nats::HeaderValue::as_str),
            Some(id),
        );
        actual.push(stored.subject.to_string());
        expected.push(format!("{}.{suffix}", address.as_str()));
    }
    let retained = stream_message_count(
        fixture.connection.jetstream(),
        fixture.topology.integration_event_stream(),
    )
    .await?;
    fixture.cleanup().await?;
    assert_eq!(
        retained, 5,
        "existing duplicate suppression remains in force"
    );
    assert_eq!(
        actual, expected,
        "physical subjects carry both envelope control IDs"
    );
    Ok(())
}

#[tokio::test]
async fn publication_derives_missing_outbound_correlation_from_the_envelope() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let envelope = event("external-message", "envelope-correlation", None)?;
    let message = outbound(&address, &envelope)?;
    assert!(message.correlation_id().is_none());
    let ack = fixture
        .connection
        .publisher(fixture.topology.clone())
        .publish_integration_event_with_ack(message, Duration::from_secs(5))
        .await?;
    let stored = integration_stream(&fixture)
        .await?
        .get_raw_message(ack.sequence())
        .await?;
    fixture.cleanup().await?;
    assert_eq!(
        stored
            .headers
            .get(publish::CORRELATION_ID_HEADER)
            .map(async_nats::HeaderValue::as_str),
        Some("envelope-correlation"),
        "the adapter-owned correlation header must agree with the required envelope",
    );
    Ok(())
}

#[tokio::test]
async fn native_filters_select_correlation_and_direct_causation_across_producer_routes()
-> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let publisher = fixture.connection.publisher(fixture.topology.clone());
    for (producer, name, id, correlation, causation) in [
        ("billing", "changed", "child-a", "flow.*", Some("root.>")),
        ("rentals", "started", "child-b", "flow.*", Some("root.>")),
        (
            "billing",
            "revoked",
            "child-c",
            "other-flow",
            Some("root.>"),
        ),
        (
            "rentals",
            "returned",
            "grandchild",
            "flow.*",
            Some("child-a"),
        ),
        (
            "billing",
            "changed",
            "uncorrelated",
            "flow.x",
            Some("root.x"),
        ),
        ("rentals", "started", "no-cause", "flow.*", None),
    ] {
        let address = fixture
            .context
            .application()
            .bounded_context(producer)?
            .integration_event_address(name)?;
        let envelope = event(id, correlation, causation)?;
        publisher
            .publish_integration_event_with_ack(
                outbound(&address, &envelope)?,
                Duration::from_secs(5),
            )
            .await?;
    }
    let prefix = fixture.context.application().as_str();
    let correlated =
        native_filtered_ids(&fixture, &format!("{prefix}.integration.*.*.flow%2E%2A.*")).await?;
    let caused =
        native_filtered_ids(&fixture, &format!("{prefix}.integration.*.*.*.root%2E%3E")).await?;
    fixture.cleanup().await?;
    assert_eq!(
        (correlated, caused),
        (
            ["child-a", "child-b", "grandchild", "no-cause"]
                .map(str::to_owned)
                .to_vec(),
            ["child-a", "child-b", "child-c"]
                .map(str::to_owned)
                .to_vec(),
        ),
        "native filters cross producer/event routes; direct causation excludes grandchildren",
    );
    Ok(())
}

#[tokio::test]
async fn maximum_valid_control_ids_remain_publishable_and_filterable() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let envelope = event(
        "external-message",
        &"%".repeat(MAX_IDENTIFIER_BYTES),
        Some(&"?".repeat(MAX_IDENTIFIER_BYTES)),
    )?;
    fixture
        .connection
        .publisher(fixture.topology.clone())
        .publish_integration_event_with_ack(outbound(&address, &envelope)?, Duration::from_secs(5))
        .await?;
    let filter = format!(
        "{}.integration.*.*.{}.*",
        fixture.context.application().as_str(),
        "%25".repeat(MAX_IDENTIFIER_BYTES),
    );
    let accepted = messaging_config::SubjectFilter::new(filter.clone());
    let selected = native_filtered_ids(&fixture, &filter).await?;
    fixture.cleanup().await?;
    assert!(
        accepted.is_ok(),
        "valid percent-encoded control IDs exceed the old 512-byte filter bound"
    );
    assert_eq!(selected, ["external-message"]);
    Ok(())
}

async fn assert_rejected_without_storage(
    fixture: Fixture,
    message: OutboundMessage<IntegrationEventAddress>,
) -> TestResult<()> {
    let before = fixture.message_counts().await?;
    let result = fixture
        .connection
        .publisher(fixture.topology.clone())
        .publish_integration_event_with_ack(message, Duration::from_secs(5))
        .await;
    let after = fixture.message_counts().await?;
    fixture.cleanup().await?;
    assert_eq!(
        (result, after),
        (Err(error::NatsError::InvalidMessage), before),
        "invalid integration publication must be rejected before broker storage",
    );
    Ok(())
}

#[tokio::test]
async fn inconsistent_outbound_correlation_is_rejected_without_storage() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let envelope = event(
        "external-message",
        "envelope-correlation",
        Some("external-cause"),
    )?;
    let message = outbound(&address, &envelope)?
        .with_correlation_id(CorrelationId::new("different-correlation")?);
    assert_rejected_without_storage(fixture, message).await
}

#[tokio::test]
async fn inconsistent_outbound_message_id_is_rejected_without_storage() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let envelope = event("envelope-message", "envelope-correlation", None)?;
    let message = OutboundMessage::json(address, MessageId::new("different-message")?, &envelope)?;
    assert_rejected_without_storage(fixture, message).await
}

#[tokio::test]
async fn non_envelopes_and_invalid_envelopes_are_rejected_without_storage() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let publisher = fixture.connection.publisher(fixture.topology.clone());
    let before = fixture.message_counts().await?;
    let valid = serde_json::to_value(event("invalid-envelope", "flow", None)?)?;
    let mut missing_correlation = valid.clone();
    missing_correlation
        .as_object_mut()
        .ok_or("envelope object")?
        .remove("correlation_id");
    missing_correlation["message_id"] = json!("missing-correlation");
    let mut invalid_schema = valid.clone();
    invalid_schema["schema_version"] = json!(0);
    invalid_schema["message_id"] = json!("invalid-schema");
    let mut invalid_causation = valid;
    invalid_causation["causation_id"] = json!("");
    invalid_causation["message_id"] = json!("invalid-causation");
    let mut results = Vec::new();
    for (id, payload) in [
        ("non-json", b"not-json".to_vec()),
        (
            "bare-payload",
            serde_json::to_vec(&json!({"business_value": 42}))?,
        ),
        (
            "missing-correlation",
            serde_json::to_vec(&missing_correlation)?,
        ),
        ("invalid-schema", serde_json::to_vec(&invalid_schema)?),
        ("invalid-causation", serde_json::to_vec(&invalid_causation)?),
    ] {
        results.push(
            publisher
                .publish_integration_event_with_ack(
                    OutboundMessage::new(address.clone(), MessageId::new(id)?, payload)?,
                    Duration::from_secs(5),
                )
                .await,
        );
    }
    let after = fixture.message_counts().await?;
    fixture.cleanup().await?;
    assert_eq!(
        (results, after),
        (
            (0..5)
                .map(|_| Err(error::NatsError::InvalidMessage))
                .collect::<Vec<_>>(),
            before,
        ),
        "publication requires a fully valid IntegrationEventEnvelope",
    );
    Ok(())
}

struct RecordingIntegrationHandler {
    deliveries: tokio::sync::mpsc::UnboundedSender<MessageDelivery<IntegrationEventAddress>>,
}

#[async_trait]
impl MessageHandler<IntegrationEventAddress> for RecordingIntegrationHandler {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let _ = self.deliveries.send(delivery);
        DeliveryDisposition::Acknowledge
    }
}

async fn logical_consumer(
    fixture: &Fixture,
    address: &IntegrationEventAddress,
) -> TestResult<(
    tokio::task::JoinHandle<Result<(), rostfrei_messaging_core::ConsumeError>>,
    tokio::sync::mpsc::UnboundedReceiver<MessageDelivery<IntegrationEventAddress>>,
)> {
    let config = ConsumerConfig::new(
        fixture.context.consumer_name("public-facts", 1)?,
        fixture.context.durable_name("public-facts", 1)?,
        address.clone(),
        Duration::from_secs(10),
        Duration::from_secs(5),
        1,
        3,
    )?;
    let provisioned =
        provision_durable_consumer(fixture.connection.jetstream(), &fixture.topology, &config)
            .await?;
    assert_eq!(provisioned.name, config.durable_name().as_str());
    let factory = fixture
        .connection
        .consumer_factory(fixture.topology.clone());
    factory.verify_consumer(&config).await?;
    let consumer =
        <NatsConsumerFactory as MessageConsumerFactory<IntegrationEventAddress>>::create(
            &factory, config,
        )?;
    let (deliveries, received) = tokio::sync::mpsc::unbounded_channel();
    let handler = Arc::new(RecordingIntegrationHandler { deliveries });
    Ok((
        tokio::spawn(async move { consumer.run(handler).await }),
        received,
    ))
}

async fn publish_raw(
    fixture: &Fixture,
    subject: String,
    id: &str,
    correlation: &str,
    payload: Vec<u8>,
) -> TestResult<()> {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert(CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE);
    headers.insert("Nats-Msg-Id", id);
    headers.insert(publish::CORRELATION_ID_HEADER, correlation);
    fixture
        .connection
        .jetstream()
        .publish_with_headers(subject, headers, payload.into())
        .await?
        .await?;
    Ok(())
}

#[tokio::test]
async fn logical_worker_receives_physical_subjects_across_control_ids() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let (task, mut received) = logical_consumer(&fixture, &address).await?;
    let mut expected = Vec::new();
    // Raw publication isolates the subscriber contract from the publisher change.
    for (id, correlation, causation, suffix) in [
        ("external-a", "flow-a", Some("cause-a"), "flow-a.cause-a"),
        ("external-b", "flow-b", None, "flow-b.~"),
        (
            "external-c",
            "flow.*",
            Some("cause.>"),
            "flow%2E%2A.cause%2E%3E",
        ),
    ] {
        let envelope = event(id, correlation, causation)?;
        let payload = serde_json::to_vec(&envelope)?;
        publish_raw(
            &fixture,
            format!("{}.{suffix}", address.as_str()),
            id,
            correlation,
            payload.clone(),
        )
        .await?;
        expected.push((
            address.clone(),
            id.to_owned(),
            Some(correlation.to_owned()),
            payload,
        ));
    }
    let mut actual = Vec::new();
    let observation = tokio::time::timeout(Duration::from_secs(3), async {
        while actual.len() < expected.len() {
            let Some(delivery) = received.recv().await else {
                break;
            };
            actual.push((
                delivery.address().clone(),
                delivery.message_id().as_str().to_owned(),
                delivery.correlation_id().map(|id| id.as_str().to_owned()),
                delivery.payload().to_vec(),
            ));
        }
    })
    .await;
    task.abort();
    let _ = task.await;
    fixture.cleanup().await?;
    assert_eq!(
        actual, expected,
        "logical worker must receive every physical suffix; observation: {observation:?}"
    );
    Ok(())
}

#[tokio::test]
async fn invalid_physical_deliveries_are_quarantined_on_the_logical_route() -> TestResult<()> {
    let fixture = Fixture::new(rostfrei_testing::integration::nats_url()?).await?;
    let address = fixture.context.integration_event_address("public-fact")?;
    let (task, mut received) = logical_consumer(&fixture, &address).await?;
    let cases = [
        ("subject-correlation", "wrong.cause", "flow", false),
        ("subject-causation", "flow.wrong", "flow", false),
        ("header-correlation", "flow.cause", "wrong", false),
        ("header-message-id", "flow.cause", "flow", false),
        ("malformed-suffix", "flow%ZZ.cause", "flow", false),
        ("missing-suffix", "flow", "flow", false),
        ("extra-suffix", "flow.cause.extra", "flow", false),
        ("non-envelope", "flow.cause", "flow", true),
    ];
    let mut expected_subjects = Vec::new();
    for (id, suffix, header_correlation, malformed_payload) in cases {
        let envelope_id = if id == "header-message-id" {
            "different-envelope-id"
        } else {
            id
        };
        let envelope = event(envelope_id, "flow", Some("cause"))?;
        let payload = if malformed_payload {
            b"not-json".to_vec()
        } else {
            serde_json::to_vec(&envelope)?
        };
        let subject = format!("{}.{suffix}", address.as_str());
        publish_raw(&fixture, subject.clone(), id, header_correlation, payload).await?;
        expected_subjects.push(subject);
    }
    let mut durable: async_nats::jetstream::consumer::PullConsumer = integration_stream(&fixture)
        .await?
        .get_consumer(fixture.context.durable_name("public-facts", 1)?.as_str())
        .await?;
    let observation = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let info = durable.info().await?;
            if stream_message_count(
                fixture.connection.jetstream(),
                fixture.topology.quarantine_stream(),
            )
            .await?
                == 8
                && info.num_pending == 0
                && info.num_ack_pending == 0
            {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    task.abort();
    let _ = task.await;
    let handled = received.try_recv().ok();
    let stream = fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.quarantine_stream().as_str())
        .await?;
    let logical_quarantine = format!(
        "{}.quarantine.integration.messaging.public-fact",
        fixture.context.application().as_str()
    );
    let mut records = Vec::new();
    for sequence in 1..=stream.cached_info().state.last_sequence {
        let stored = stream.get_raw_message(sequence).await?;
        let record: QuarantineRecord = serde_json::from_slice(&stored.payload)?;
        records.push((
            stored.subject.to_string(),
            record.address().to_owned(),
            record.failure_kind(),
        ));
    }
    fixture.cleanup().await?;
    assert!(
        handled.is_none(),
        "invalid transport identities must not reach the handler"
    );
    assert_eq!(
        records,
        expected_subjects
            .into_iter()
            .map(|physical| (
                logical_quarantine.clone(),
                physical,
                Some(QuarantineFailureKind::InvalidSourceMessage),
            ))
            .collect::<Vec<_>>(),
        "quarantine retains actual physical subjects and uses the logical destination; observation: {observation:?}",
    );
    assert!(
        matches!(observation, Ok(Ok(()))),
        "quarantined source deliveries must be terminated, not repeatedly NAKed: {observation:?}",
    );
    Ok(())
}
