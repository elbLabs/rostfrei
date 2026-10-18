//! Second independent review: additional protocol negatives and prepared-publication controls.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use rostfrei::EncodedCommand;

fn command(f: &Fixture, operation: &str) -> TestResult<EncodedCommand> {
    local_bus(
        &f.context,
        processor(
            &f.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    )
    .encode(request(operation, "target", "noop")?)
    .map_err(Into::into)
}
fn diagnostic(
    c: &EncodedCommand,
    kind: StoreFailureKind,
) -> TestResult<OutboundMessage<CommandResponseAddress>> {
    let failure = ExecutionFailure::Store(kind);
    let response = CommandResponse::execution_failed(
        rostfrei::command_technical_response_message_id(c.message_id(), failure)?,
        c.message_id().clone(),
        c.address().clone(),
        c.operation_id().clone(),
        c.correlation_id().clone(),
        failure,
    )?;
    Ok(OutboundMessage::json(
        c.response_address()?,
        response.message_id().clone(),
        &response,
    )?)
}

#[test]
fn duplicate_known_response_fields_must_not_be_collapsed_before_validation() {
    let prefix = r#""message_id":"response","command_message_id":"command","command_address":"acme.command.orders.place-order","operation_id":"operation","correlation_id":"correlation""#;
    let wires = [
        format!(
            "{{{prefix},\"message_id\":\"another-response\",\"schema_version\":1,\"outcome\":{{\"status\":\"accepted\"}}}}"
        ),
        format!(
            "{{{prefix},\"schema_version\":99,\"schema_version\":2,\"outcome\":{{\"status\":\"execution_failed\",\"value\":{{\"source\":\"store\",\"kind\":\"unavailable\"}}}}}}"
        ),
        format!(
            "{{{prefix},\"schema_version\":2,\"outcome\":{{\"status\":\"accepted\",\"status\":\"execution_failed\",\"value\":{{\"source\":\"store\",\"kind\":\"unavailable\"}}}}}}"
        ),
        format!(
            "{{{prefix},\"schema_version\":2,\"outcome\":{{\"status\":\"execution_failed\",\"value\":{{\"source\":\"store\",\"kind\":\"corrupt_history\",\"kind\":\"unavailable\"}}}}}}"
        ),
    ];
    assert!(
        serde_json::from_str::<ExecutionFailure>(
            r#"{"source":"store","kind":"corrupt_history","kind":"unavailable"}"#
        )
        .is_err(),
        "direct typed source decoder rejects ambiguous fields"
    );
    let mut refused = Vec::new();
    for (index, wire) in wires.iter().enumerate() {
        let result = serde_json::from_str::<CommandResponse>(wire);
        println!("duplicate wire {index}: {result:?}");
        refused.push(result.is_err());
    }
    assert!(
        refused.iter().all(|refused| *refused),
        "known duplicate fields must fail closed, not select their last value: {refused:?}"
    );
}

fn token_headers(
    c: &EncodedCommand,
    message: &OutboundMessage<CommandResponseAddress>,
    token: &DiagnosticTransitionToken,
) -> TestResult<async_nats::HeaderMap> {
    use sha2::{Digest, Sha256};
    let encoded = serde_json::to_vec(token)?;
    let subject = c.response_address()?.to_string();
    let mut hash = Sha256::new();
    for frame in [
        b"rostfrei:diagnostic-publication:v1".as_slice(),
        subject.as_bytes(),
        message.payload(),
        encoded.as_slice(),
    ] {
        hash.update(u64::try_from(frame.len())?.to_be_bytes());
        hash.update(frame);
    }
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert("Nats-Msg-Id", format!("{:x}", hash.finalize()));
    headers.insert(DIAGNOSTIC_TRANSITION_HEADER, String::from_utf8(encoded)?);
    Ok(headers)
}

#[tokio::test]
async fn native_writer_must_not_store_a_diagnostic_with_conflicting_duplicate_failure_kinds()
-> TestResult {
    let f = Fixture::new("r2-native-duplicate-kind").await?;
    let c = command(&f, "operation")?;
    let message = diagnostic(&c, StoreFailureKind::Unavailable)?;
    let raw = std::str::from_utf8(message.payload())?;
    let ambiguous = raw.replace(
        "\"kind\":\"unavailable\"",
        "\"kind\":\"corrupt_history\",\"kind\":\"unavailable\"",
    );
    assert_ne!(
        raw, ambiguous,
        "fixture inserts actual duplicate JSON keys, not a Value map"
    );
    let message = OutboundMessage::new(
        c.response_address()?,
        message.message_id().clone(),
        ambiguous.into_bytes(),
    )?;
    let prepared = f
        .publisher()
        .prepare_command_response(message, Duration::from_secs(2))
        .await;
    let refused = prepared.is_err();
    if let Ok(prepared) = prepared {
        let ack = f
            .publisher()
            .publish_prepared_command_response(&prepared, Duration::from_secs(2))
            .await?;
        let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
        let response = reader
            .read_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2),
            )
            .await?;
        println!(
            "ambiguous raw failure kinds stored sequence={}, reader selected={:?}",
            ack.sequence(),
            response.outcome()
        );
        assert_eq!(
            response.outcome(),
            &CommandResponseOutcome::ExecutionFailed(ExecutionFailure::Store(
                StoreFailureKind::Unavailable
            ))
        );
    }
    assert!(
        refused,
        "a contradictory corrupt_history/unavailable diagnostic must fail before durable publication"
    );
    Ok(())
}

#[tokio::test]
async fn technical_reader_must_validate_outer_correlation_as_well_as_payload_token() -> TestResult {
    let f = Fixture::new("r2-outer-correlation").await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let mut refused = Vec::new();
    for mode in ["valid", "absent", "contradictory", "duplicate"] {
        let c = command(&f, mode)?;
        let message = diagnostic(&c, StoreFailureKind::CorruptHistory)?;
        let prepared = f
            .publisher()
            .prepare_command_response(message.clone(), Duration::from_secs(2))
            .await?;
        let mut headers =
            token_headers(&c, &message, prepared.transition().ok_or("missing token")?)?;
        if mode != "absent" {
            headers.insert(
                CORRELATION_ID_HEADER,
                if mode == "contradictory" {
                    "different-correlation"
                } else {
                    c.correlation_id().as_str()
                },
            );
        }
        if mode == "duplicate" {
            headers.append(CORRELATION_ID_HEADER, c.correlation_id().as_str());
        }
        f.js.publish_with_headers(
            c.response_address()?.to_string(),
            headers,
            message.payload().to_vec().into(),
        )
        .await?
        .await?;
        let result = reader
            .find_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2),
            )
            .await;
        println!("outer correlation {mode}: {result:?}");
        if mode == "valid" {
            assert!(result?.is_some());
        } else {
            refused.push(result.is_err());
        }
    }
    assert!(
        refused.iter().all(|refused| *refused),
        "technical readers must refuse missing/contradictory/duplicate outer correlation: {refused:?}"
    );
    Ok(())
}

#[tokio::test]
async fn unrelated_subject_progress_does_not_invalidate_an_unchanged_prepared_request() -> TestResult
{
    let f = Fixture::new("r2-unrelated-progress").await?;
    let c = command(&f, "first")?;
    let other = command(&f, "other")?;
    let p = f.publisher();
    let message = diagnostic(&c, StoreFailureKind::Unavailable)?;
    let prepared = p
        .prepare_command_response(message, Duration::from_secs(2))
        .await?;
    let other_ack = p
        .publish_command_response_with_ack(
            diagnostic(&other, StoreFailureKind::Conflict)?,
            Duration::from_secs(2),
        )
        .await?;
    let ack = p
        .publish_prepared_command_response(&prepared, Duration::from_secs(2))
        .await?;
    assert!(!ack.duplicate());
    assert!(ack.sequence() > other_ack.sequence());
    assert!(
        p.publish_prepared_command_response(&prepared, Duration::from_secs(2))
            .await?
            .duplicate()
    );
    Ok(())
}

#[tokio::test]
async fn sequential_stream_recreation_refuses_old_prepared_epoch_and_accepts_fresh_epoch()
-> TestResult {
    let f = Fixture::new("r2-stream-epoch").await?;
    let c = command(&f, "operation")?;
    let message = diagnostic(&c, StoreFailureKind::Unavailable)?;
    let p = f.publisher();
    let old = p
        .prepare_command_response(message.clone(), Duration::from_secs(2))
        .await?;
    let stream =
        f.js.get_stream(f.messaging.topology().command_response_stream().as_str())
            .await?;
    let config = stream.cached_info().config.clone();
    f.js.delete_stream(f.messaging.topology().command_response_stream().as_str())
        .await?;
    f.js.create_stream(config).await?;
    assert!(
        p.publish_prepared_command_response(&old, Duration::from_secs(2))
            .await
            .is_err()
    );
    let fresh = p
        .prepare_command_response(message, Duration::from_secs(2))
        .await?;
    assert_ne!(fresh.publication_message_id(), old.publication_message_id());
    p.publish_prepared_command_response(&fresh, Duration::from_secs(2))
        .await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    assert!(
        reader
            .find_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2)
            )
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn same_and_different_prepared_writers_only_reconcile_exact_publication_identity()
-> TestResult {
    let fixture = Fixture::new("r2-prepared-races").await?;
    let publisher = fixture.publisher();
    for same in [true, false] {
        let encoded = command(&fixture, if same { "same" } else { "different" })?;
        let first_prepared = publisher
            .prepare_command_response(
                diagnostic(&encoded, StoreFailureKind::Unavailable)?,
                Duration::from_secs(2),
            )
            .await?;
        let second_prepared = publisher
            .prepare_command_response(
                diagnostic(
                    &encoded,
                    if same {
                        StoreFailureKind::Unavailable
                    } else {
                        StoreFailureKind::Conflict
                    },
                )?,
                Duration::from_secs(2),
            )
            .await?;
        let (first, second) = tokio::join!(
            publisher.publish_prepared_command_response(&first_prepared, Duration::from_secs(2)),
            publisher.publish_prepared_command_response(&second_prepared, Duration::from_secs(2))
        );
        println!("same={same}, prepared writer results={first:?} / {second:?}");
        if same {
            assert!(first.is_ok() && second.is_ok());
            assert_ne!(first?.duplicate(), second?.duplicate());
        } else {
            assert!(
                first.is_ok() ^ second.is_ok(),
                "conflicting snapshots need one CAS winner, not success for every writer"
            );
        }
    }
    Ok(())
}

struct PublishGateProxy(tokio::task::JoinHandle<TestResult>);

#[tokio::test]
async fn prepared_business_response_keeps_schema_one_and_cannot_be_misrouted_or_replaced()
-> TestResult {
    let f = Fixture::new("r2-business-scope").await?;
    let c = command(&f, "business")?;
    let response = CommandResponse::accepted(
        rostfrei::command_response_message_id(c.message_id())?,
        c.message_id().clone(),
        c.address().clone(),
        c.operation_id().clone(),
        c.correlation_id().clone(),
    )?;
    assert_eq!(response.schema_version().get(), 1);
    let message = OutboundMessage::json(
        c.response_address()?,
        response.message_id().clone(),
        &response,
    )?;
    let wrong = derive_command_response_address(
        c.address(),
        c.operation_id(),
        &MessageId::new("wrong-command-message")?,
    )?;
    let invalid = OutboundMessage::new(
        wrong,
        message.message_id().clone(),
        message.payload().to_vec(),
    )?;
    assert!(
        f.publisher()
            .prepare_command_response(invalid, Duration::from_secs(2))
            .await
            .is_err()
    );
    let prepared = f
        .publisher()
        .prepare_command_response(message, Duration::from_secs(2))
        .await?;
    assert!(prepared.transition().is_none());
    assert_eq!(prepared.publication_message_id(), response.message_id());
    let foreign = NatsPublisher::new(
        f.js.clone(),
        MessagingTopology::for_application(&ApplicationName::new("foreign-response-app")?)?,
    );
    assert!(
        foreign
            .publish_prepared_command_response(&prepared, Duration::from_secs(2))
            .await
            .is_err()
    );
    let first = f
        .publisher()
        .publish_prepared_command_response(&prepared, Duration::from_secs(2))
        .await?;
    assert!(!first.duplicate());
    assert!(
        f.publisher()
            .publish_prepared_command_response(&prepared, Duration::from_secs(2))
            .await?
            .duplicate()
    );
    assert!(
        f.publisher()
            .prepare_command_response(
                diagnostic(&c, StoreFailureKind::CorruptHistory)?,
                Duration::from_secs(2)
            )
            .await
            .is_err()
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    assert_eq!(
        reader
            .read_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2)
            )
            .await?,
        response
    );
    Ok(())
}

impl Drop for PublishGateProxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn publication_gate_proxy(
    subject: String,
) -> TestResult<(
    PublishGateProxy,
    std::net::SocketAddr,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Semaphore>,
)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = std::env::var("ROSTFREI_NATS_URL")?;
    let upstream = url
        .rsplit('@')
        .next()
        .ok_or("missing fixture upstream")?
        .to_owned();
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let notify = reached.clone();
    let gate = release.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream).await?;
        let (client_read, mut client_write) = client.into_split();
        let (mut upstream_read, mut upstream_write) = upstream.into_split();
        let upload = async {
            let mut reader = BufReader::new(client_read);
            let mut gated = false;
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await? == 0 {
                    return TestResult::Ok(());
                }
                let text = std::str::from_utf8(&line)?;
                if text.starts_with("PUB ") || text.starts_with("HPUB ") {
                    let parts: Vec<_> = text.split_whitespace().collect();
                    let length: usize = parts.last().ok_or("no publication length")?.parse()?;
                    let mut body =
                        vec![0; length.checked_add(2).ok_or("publication length overflow")?];
                    reader.read_exact(&mut body).await?;
                    if !gated && parts.get(1) == Some(&subject.as_str()) {
                        gated = true;
                        notify.notify_one();
                        gate.acquire().await?.forget();
                    }
                    upstream_write.write_all(&line).await?;
                    upstream_write.write_all(&body).await?;
                } else {
                    upstream_write.write_all(&line).await?;
                }
            }
        };
        let download = async {
            tokio::io::copy(&mut upstream_read, &mut client_write).await?;
            TestResult::Ok(())
        };
        tokio::select! {r=upload=>r,r=download=>r}
    });
    Ok((PublishGateProxy(task), address, reached, release))
}

#[tokio::test]
async fn recreation_after_epoch_check_must_not_acknowledge_an_unreadable_diagnostic() -> TestResult
{
    let f = Fixture::new("r2-epoch-check-race").await?;
    let c = command(&f, "operation")?;
    let (proxy, address, reached, release) =
        publication_gate_proxy(c.response_address()?.to_string()).await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{address}"))
    .await?;
    let publisher = NatsPublisher::new(
        async_nats::jetstream::new(client.clone()),
        f.messaging.topology().clone(),
    );
    let prepared = publisher
        .prepare_command_response(
            diagnostic(&c, StoreFailureKind::CorruptHistory)?,
            Duration::from_secs(3),
        )
        .await?;
    assert_eq!(prepared.transition().ok_or("no token")?.generation(), 0);
    let task = tokio::spawn(async move {
        publisher
            .publish_prepared_command_response(&prepared, Duration::from_secs(5))
            .await
    });
    // The proxy gates the actual HPUB only AFTER the publisher's live epoch check has returned.
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await?;
    let stream =
        f.js.get_stream(f.messaging.topology().command_response_stream().as_str())
            .await?;
    let config = stream.cached_info().config.clone();
    f.js.delete_stream(f.messaging.topology().command_response_stream().as_str())
        .await?;
    f.js.create_stream(config).await?;
    release.add_permits(1);
    let publish = task.await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let retained = reader
        .find_command_response(
            &c.response_address()?,
            c.operation_id(),
            c.message_id(),
            Duration::from_secs(2),
        )
        .await;
    println!("epoch-check race: publish={publish:?}, new reader={retained:?}");
    client.drain().await?;
    drop(proxy);
    assert!(
        publish.is_err(),
        "a PubAck in the recreated stream must not claim durability for an old-epoch unreadable diagnostic"
    );
    Ok(())
}

rostfrei::install_macro_support!();
