//! Independent checks of the changed response codec/storage and fresh-Ack compatibility.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use release_contract_support::*;
use serde::Deserialize;

// Exercise the private response-only helper unchanged, exactly as the existing source-importing
// native fixture does. No other message family or production source is altered.
mod error {
    pub use rostfrei_nats::NatsError;
}
#[path = "../src/response_storage.rs"]
mod response_storage;

#[derive(Deserialize)]
struct LegacyEnvelope {
    message_id: MessageId,
    command_message_id: MessageId,
    command_address: CommandAddress,
    operation_id: rostfrei_messaging_core::OperationId,
    schema_version: SchemaVersion,
    correlation_id: CorrelationId,
    outcome: CommandResponseOutcome,
}

#[test]
fn legacy_known_fields_and_extension_values_keep_their_previous_typed_semantics() -> TestResult {
    let fields = r#""message_id":"response","command_message_id":"command","command_address":"acme.command.orders.place-order","operation_id":"operation","schema_version":1,"correlation_id":"correlation""#;
    let outcomes = [
        r#"{"status":"accepted"}"#,
        r#"{"extension":{"status":"not-a-protocol-field","status":false},"status":"accepted","extension":null}"#,
        r#"{"value":null,"status":"accepted"}"#,
        r#"{"status":"accepted","value":{"application_owned":true}}"#,
        r#"{"status":"rejected","value":{"classification":"not_found","code":"test.missing","message":"missing","details":{"source":"one","source":"two","schema_version":999}}}"#,
        r#"{"value":{"details":null,"message":"missing","code":"test.missing","classification":"not_found","extension":[1,{},false]},"status":"rejected","extension":true}"#,
    ];
    for outcome in outcomes {
        for outcome_first in [true, false] {
            let raw = if outcome_first {
                format!(
                    "{{\"outcome\":{outcome},\"extension\":{{\"message_id\":null,\"message_id\":true}},{fields}}}"
                )
            } else {
                format!(
                    "{{{fields},\"outcome\":{outcome},\"extension\":[false,null,{{\"nested\":1}}]}}"
                )
            };
            let legacy = serde_json::from_str::<LegacyEnvelope>(&raw);
            let current = serde_json::from_str::<CommandResponse>(&raw);
            assert_eq!(
                current.is_ok(),
                legacy.is_ok(),
                "changed legacy interpretation: {raw}"
            );
            if let Ok(legacy) = legacy {
                let current = current?;
                assert_eq!(current.message_id(), &legacy.message_id);
                assert_eq!(current.command_message_id(), &legacy.command_message_id);
                assert_eq!(current.command_address(), &legacy.command_address);
                assert_eq!(current.operation_id(), &legacy.operation_id);
                assert_eq!(current.schema_version(), legacy.schema_version);
                assert_eq!(current.correlation_id(), &legacy.correlation_id);
                assert_eq!(current.outcome(), &legacy.outcome);
            }
        }
    }
    Ok(())
}

#[test]
fn escaped_duplicate_source_and_rejection_keys_remain_known_even_in_content_first_order() {
    let fields = r#""message_id":"response","command_message_id":"command","command_address":"acme.command.orders.place-order","operation_id":"operation","correlation_id":"correlation""#;
    for outcome in [
        r#"{"value":{"kind":"unavailable","\u006bind":"unavailable","source":"store"},"status":"execution_failed"}"#,
        r#"{"value":{"source":"store","\u0073ource":"store","kind":"unavailable"},"status":"execution_failed"}"#,
        r#"{"value":{"kind":"unavailable","source":"store"},"status":"execution_failed","\u0073tatus":"execution_failed"}"#,
        r#"{"value":{"kind":"unavailable","source":"store"},"\u0076alue":{"kind":"unavailable","source":"store"},"status":"execution_failed"}"#,
    ] {
        let raw = format!("{{\"outcome\":{outcome},{fields},\"schema_version\":2}}");
        assert!(
            serde_json::from_str::<CommandResponse>(&raw).is_err(),
            "ambiguous typed field: {raw}"
        );
    }
    let raw = format!(
        r#"{{{fields},"schema_version":1,"outcome":{{"value":{{"classification":"not_found","code":"test.missing","message":"missing","details":null,"\u0064etails":null}},"status":"rejected"}}}}"#
    );
    assert!(serde_json::from_str::<CommandResponse>(&raw).is_err());
}

static REPLY_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
async fn mock_storage_reply(
    fixture: &Fixture,
    reply: Vec<u8>,
    by_sequence: bool,
) -> TestResult<Result<Option<async_nats::jetstream::message::StreamMessage>, NatsError>> {
    let prefix = format!(
        "REVIEW.STORAGE.{}.{}",
        std::process::id(),
        REPLY_SEQUENCE.fetch_add(1, Ordering::SeqCst)
    );
    let client = fixture.js.client().clone();
    let mut subscription = client
        .subscribe(format!("{prefix}.STREAM.MSG.GET.RESPONSE_TEST"))
        .await?;
    client.flush().await?;
    let responder = client.clone();
    let task = tokio::spawn(async move {
        let request = subscription
            .next()
            .await
            .ok_or("mock request not delivered")?;
        let expected = if by_sequence {
            serde_json::json!({"seq":1})
        } else {
            serde_json::json!({"last_by_subj":"test.subject"})
        };
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request.payload)?,
            expected
        );
        responder
            .publish(request.reply.ok_or("request has no reply")?, reply.into())
            .await?;
        responder.flush().await?;
        TestResult::Ok(())
    });
    let context = async_nats::jetstream::with_prefix(client, &prefix);
    let result = if by_sequence {
        response_storage::sequence(&context, "RESPONSE_TEST", 1).await
    } else {
        response_storage::latest(&context, "RESPONSE_TEST", "test.subject").await
    };
    tokio::time::timeout(Duration::from_secs(3), task).await???;
    Ok(result)
}

fn raw_record(headers: Option<&[u8]>, payload: &[u8]) -> serde_json::Value {
    let mut record = serde_json::json!({"subject":"test.subject","seq":1,"time":"2026-10-15T00:00:00Z","data":STANDARD.encode(payload)});
    if let Some(headers) = headers {
        record.as_object_mut().unwrap().insert(
            "hdrs".to_owned(),
            serde_json::json!(STANDARD.encode(headers)),
        );
    }
    serde_json::json!({"message":record})
}

#[tokio::test]
async fn raw_storage_keeps_identical_duplicates_folding_and_pinned_header_name_semantics()
-> TestResult {
    let fixture = Fixture::new("r3-storage-header-compat").await?;
    let headers=b"NATS/1.0 200 Stored\r\nrostfrei-Control-Correlation-Id: same\r\nrostfrei-Control-Correlation-Id: same\r\nrostfrei-control-correlation-id: distinct-name\r\nNats-Msg-Id: one\r\nNats-Msg-Id: two\r\nX-Note: first\r\n continuation\r\n\r\n";
    for by_sequence in [true, false] {
        let record = mock_storage_reply(
            &fixture,
            serde_json::to_vec(&raw_record(Some(headers), b"bytes"))?,
            by_sequence,
        )
        .await??
        .ok_or("missing valid response")?;
        assert_eq!(record.payload.as_ref(), b"bytes");
        assert_eq!(
            record
                .headers
                .get_all(CORRELATION_ID_HEADER)
                .map(async_nats::HeaderValue::as_str)
                .collect::<Vec<_>>(),
            vec!["same", "same"]
        );
        assert_eq!(record.headers.get_all("Nats-Msg-Id").count(), 2);
        assert_eq!(
            record
                .headers
                .get("X-Note")
                .map(async_nats::HeaderValue::as_str),
            Some("first continuation")
        );
        // Compare to the pinned HeaderName behavior; do not import HTTP case folding into NATS.
        let canonical: async_nats::HeaderName = CORRELATION_ID_HEADER.parse()?;
        let alternate: async_nats::HeaderName = "rostfrei-control-correlation-id".parse()?;
        assert_ne!(canonical, alternate);
        assert_eq!(
            record
                .headers
                .get("rostfrei-control-correlation-id")
                .map(async_nats::HeaderValue::as_str),
            Some("distinct-name")
        );
    }
    let record = mock_storage_reply(
        &fixture,
        serde_json::to_vec(&raw_record(None, b"plain"))?,
        false,
    )
    .await??
    .ok_or("missing no-header record")?;
    assert!(record.headers.is_empty());
    Ok(())
}

#[tokio::test]
async fn raw_storage_refuses_malformed_header_base64_status_payload_and_api_replies() -> TestResult
{
    let fixture = Fixture::new("r3-storage-refusal").await?;
    let valid = raw_record(
        Some(b"NATS/1.0\r\nContent-Type: application/json\r\n\r\n"),
        b"payload",
    );
    let mut bad_header_encoding = valid.clone();
    bad_header_encoding["message"]["hdrs"] = serde_json::json!("not base64!");
    let mut bad_payload_encoding = valid.clone();
    bad_payload_encoding["message"]["data"] = serde_json::json!("not base64!");
    let mut cases = vec![bad_header_encoding, bad_payload_encoding];
    for header in [
        b"WRONG/1.0\r\nX: value\r\n\r\n".as_slice(),
        b"NATS/1.0 invalid-status\r\nX: value\r\n\r\n".as_slice(),
        b"NATS/1.0\r\nmissing-colon\r\n\r\n".as_slice(),
        b"NATS/1.0\r\nbad name: value\r\n\r\n".as_slice(),
        b"NATS/1.0\r\nX: \xff\r\n\r\n".as_slice(),
    ] {
        cases.push(raw_record(Some(header), b"payload"));
    }
    for value in cases {
        assert!(
            mock_storage_reply(&fixture, serde_json::to_vec(&value)?, false)
                .await?
                .is_err()
        );
    }
    let absent =
        serde_json::json!({"error":{"code":404,"err_code":10037,"description":"no message found"}});
    assert!(
        mock_storage_reply(&fixture, serde_json::to_vec(&absent)?, true)
            .await??
            .is_none()
    );
    for value in [
        serde_json::json!({"error":{"code":404,"err_code":10059,"description":"stream not found"}}),
        serde_json::json!({"error":{"code":503,"err_code":10008,"description":"temporarily unavailable"}}),
    ] {
        assert!(
            mock_storage_reply(&fixture, serde_json::to_vec(&value)?, false)
                .await?
                .is_err(),
            "other API errors are not absence"
        );
    }
    assert!(
        mock_storage_reply(&fixture, b"not-json".to_vec(), false)
            .await?
            .is_err()
    );
    Ok(())
}

fn command(fixture: &Fixture, operation: &str) -> TestResult<rostfrei::EncodedCommand> {
    local_bus(
        &fixture.context,
        processor(
            &fixture.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    )
    .encode(request(operation, "target", "noop")?)
    .map_err(Into::into)
}

#[tokio::test]
async fn public_reader_still_refuses_raw_payload_above_the_envelope_limit() -> TestResult {
    let fixture = Fixture::new("r3-response-size").await?;
    let command = command(&fixture, "oversize")?;
    let response = CommandResponse::accepted(
        rostfrei::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
    )?;
    let mut payload = serde_json::to_vec(&response)?;
    payload.resize(
        MAX_ENVELOPE_BYTES.checked_add(1).ok_or("size overflow")?,
        b' ',
    );
    // Raise only this fixture's transport allowance to inject a record the framework must refuse.
    let stream = fixture
        .js
        .get_stream(
            fixture
                .messaging
                .topology()
                .command_response_stream()
                .as_str(),
        )
        .await?;
    let mut config = stream.cached_info().config.clone();
    config.max_message_size = 2_000_000;
    fixture.js.update_stream(config).await?;
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert("Nats-Msg-Id", response.message_id().as_str());
    fixture
        .js
        .publish_with_headers(
            command.response_address()?.to_string(),
            headers,
            payload.into(),
        )
        .await?
        .await?;
    let reader =
        NatsCommandResponseReader::new(fixture.js.clone(), fixture.messaging.topology().clone());
    assert_eq!(
        reader
            .find_command_response(
                &command.response_address()?,
                command.operation_id(),
                command.message_id(),
                Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .kind(),
        CommandResponseReadErrorKind::InvalidResponse
    );
    Ok(())
}

struct AckGate(tokio::task::JoinHandle<TestResult>);
impl Drop for AckGate {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn hold_actual_ack(
    stream_name: String,
) -> TestResult<(
    AckGate,
    std::net::SocketAddr,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Semaphore>,
)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = std::env::var("ROSTFREI_NATS_URL")?;
    let upstream = url.rsplit('@').next().ok_or("no upstream")?.to_owned();
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let notify = reached.clone();
    let gate = release.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream).await?;
        let (mut client_read, mut client_write) = client.into_split();
        let (upstream_read, mut upstream_write) = upstream.into_split();
        let upload = async {
            tokio::io::copy(&mut client_read, &mut upstream_write).await?;
            TestResult::Ok(())
        };
        let download = async {
            let mut reader = BufReader::new(upstream_read);
            let mut held = false;
            let signature = format!("\"stream\":\"{stream_name}\"");
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await? == 0 {
                    return TestResult::Ok(());
                }
                let text = std::str::from_utf8(&line)?;
                if text.starts_with("MSG ") || text.starts_with("HMSG ") {
                    let length: usize = text
                        .split_whitespace()
                        .last()
                        .ok_or("no frame length")?
                        .parse()?;
                    let mut body = vec![0; length.checked_add(2).ok_or("frame overflow")?];
                    reader.read_exact(&mut body).await?;
                    if !held
                        && body
                            .windows(signature.len())
                            .any(|window| window == signature.as_bytes())
                    {
                        held = true;
                        notify.notify_one();
                        gate.acquire().await?.forget();
                    }
                    client_write.write_all(&line).await?;
                    client_write.write_all(&body).await?;
                } else {
                    client_write.write_all(&line).await?;
                }
            }
        };
        tokio::select! {r=upload=>r,r=download=>r}
    });
    Ok((AckGate(task), address, reached, release))
}

fn accepted(
    command: &rostfrei::EncodedCommand,
) -> TestResult<OutboundMessage<CommandResponseAddress>> {
    let response = CommandResponse::accepted(
        rostfrei::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
    )?;
    Ok(OutboundMessage::json(
        command.response_address()?,
        response.message_id().clone(),
        &response,
    )?)
}

#[tokio::test]
async fn fresh_ack_validates_its_exact_sequence_even_after_unrelated_and_terminal_progress()
-> TestResult {
    let fixture = Fixture::new("r3-valid-progress").await?;
    let original = command(&fixture, "original")?;
    let other = command(&fixture, "other")?;
    let failure = ExecutionFailure::Store(StoreFailureKind::Unavailable);
    let response = CommandResponse::execution_failed(
        rostfrei::command_technical_response_message_id(original.message_id(), failure)?,
        original.message_id().clone(),
        original.address().clone(),
        original.operation_id().clone(),
        original.correlation_id().clone(),
        failure,
    )?;
    let message = OutboundMessage::json(
        original.response_address()?,
        response.message_id().clone(),
        &response,
    )?;
    let (proxy, address, reached, release) = hold_actual_ack(
        fixture
            .messaging
            .topology()
            .command_response_stream()
            .as_str()
            .to_owned(),
    )
    .await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{address}"))
    .await?;
    let publisher = NatsPublisher::new(
        async_nats::jetstream::new(client.clone()),
        fixture.messaging.topology().clone(),
    );
    let prepared = publisher
        .prepare_command_response(message, Duration::from_secs(2))
        .await?;
    let in_flight = prepared.clone();
    let in_flight_publisher = publisher.clone();
    let task = tokio::spawn(async move {
        in_flight_publisher
            .publish_prepared_command_response(&in_flight, Duration::from_secs(5))
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await?;
    let unrelated = fixture
        .publisher()
        .publish_command_response_with_ack(accepted(&other)?, Duration::from_secs(2))
        .await?;
    let terminal = fixture
        .publisher()
        .publish_command_response_with_ack(accepted(&original)?, Duration::from_secs(2))
        .await?;
    assert!(terminal.sequence() > unrelated.sequence());
    release.add_permits(1);
    let first = task.await??;
    assert!(!first.duplicate());
    assert!(first.sequence() < unrelated.sequence());
    assert!(
        publisher
            .publish_prepared_command_response(&prepared, Duration::from_secs(2))
            .await
            .is_err(),
        "an uncertain duplicate retry still cannot rebase across a terminal winner"
    );
    let reader =
        NatsCommandResponseReader::new(fixture.js.clone(), fixture.messaging.topology().clone());
    assert_eq!(
        reader
            .read_command_response(
                &original.response_address()?,
                original.operation_id(),
                original.message_id(),
                Duration::from_secs(2)
            )
            .await?
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    println!(
        "held fresh PubAck seq={} remains valid with unrelated seq={} and terminal seq={}; stale duplicate refused",
        first.sequence(),
        unrelated.sequence(),
        terminal.sequence()
    );
    client.drain().await?;
    drop(proxy);
    Ok(())
}

rostfrei::install_macro_support!();
