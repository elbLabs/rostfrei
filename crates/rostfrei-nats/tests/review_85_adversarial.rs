//! Independent reviewer tests, not candidate implementation/acceptance history.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use rostfrei::{
    EncodedCommand, command_response_message_id, command_technical_response_message_id,
};

fn diagnostic(
    command: &EncodedCommand,
    kind: StoreFailureKind,
) -> TestResult<OutboundMessage<CommandResponseAddress>> {
    let failure = ExecutionFailure::Store(kind);
    let response = CommandResponse::execution_failed(
        command_technical_response_message_id(command.message_id(), failure)?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        failure,
    )?;
    Ok(OutboundMessage::json(
        command.response_address()?,
        response.message_id().clone(),
        &response,
    )?)
}

fn accepted(command: &EncodedCommand) -> TestResult<OutboundMessage<CommandResponseAddress>> {
    let response = CommandResponse::accepted(
        command_response_message_id(command.message_id())?,
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

fn command(f: &Fixture) -> TestResult<(CommandBus, EncodedCommand)> {
    let bus = local_bus(
        &f.context,
        processor(
            &f.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    );
    let encoded = bus.encode(request("review-operation", "target", "noop")?)?;
    Ok((bus, encoded))
}

#[tokio::test]
async fn redrive_must_refuse_changed_payload_using_an_old_message_id() -> TestResult {
    let f = Fixture::new("review-redrive-evidence").await?;
    let (bus, original) = command(&f)?;
    f.publisher()
        .publish_command_response(diagnostic(&original, StoreFailureKind::CorruptHistory)?)
        .await?;
    let mut wire: serde_json::Value = serde_json::from_slice(original.payload())?;
    wire["payload"]["payload"]["target"] = serde_json::json!("changed-target");
    let changed = EncodedCommand::from_delivery(
        original.address().clone(),
        original.message_id().clone(),
        serde_json::to_vec(&wire)?,
    )?;
    assert!(
        bus.dispatch_encoded(changed.clone()).await.is_err(),
        "control: dispatcher must detect the invalid canonical identity"
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let result = reader
        .remove_blocked_diagnostic_for_redrive(&changed, Duration::from_secs(2))
        .await;
    let retained = reader
        .find_command_response(
            &original.response_address()?,
            original.operation_id(),
            original.message_id(),
            Duration::from_secs(2),
        )
        .await?;
    println!(
        "changed-evidence redrive result={result:?}, original diagnostic retained={}",
        retained.is_some()
    );
    assert!(
        result.is_err(),
        "invalid original evidence must not authorize diagnostic deletion"
    );
    assert!(retained.is_some());
    Ok(())
}

#[tokio::test]
async fn repeated_diagnostic_source_must_supersede_the_latest_transient_snapshot() -> TestResult {
    let f = Fixture::new("review-cycle").await?;
    let (_, command) = command(&f)?;
    let a = diagnostic(&command, StoreFailureKind::Unavailable)?;
    let b = diagnostic(&command, StoreFailureKind::Conflict)?;
    let first = f
        .publisher()
        .publish_command_response_with_ack(a.clone(), Duration::from_secs(2))
        .await?;
    let second = f
        .publisher()
        .publish_command_response_with_ack(b, Duration::from_secs(2))
        .await?;
    let third = f
        .publisher()
        .publish_command_response_with_ack(a, Duration::from_secs(2))
        .await;
    println!(
        "A->B->A seq first={} second={} final={third:?}",
        first.sequence(),
        second.sequence()
    );
    // The blocked cycle must not be mistaken for a general sink failure or inability to complete.
    let completed = f
        .publisher()
        .publish_command_response_with_ack(accepted(&command)?, Duration::from_secs(2))
        .await?;
    assert!(completed.sequence() > second.sequence());
    assert!(
        third.is_ok(),
        "the current transient diagnostic must be durable, even if its source recurs within deduplication"
    );
    Ok(())
}

#[tokio::test]
async fn cleared_diagnostic_must_be_republishable_after_nonduplicate_command_puback() -> TestResult
{
    let f = Fixture::new("review-redrive-dedup").await?;
    let (_, command) = command(&f)?;
    let diagnostic = diagnostic(&command, StoreFailureKind::CapacityExhausted)?;
    f.publisher()
        .publish_command_response(diagnostic.clone())
        .await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    assert!(
        reader
            .remove_blocked_diagnostic_for_redrive(&command, Duration::from_secs(2))
            .await?
    );
    let source = f
        .publisher()
        .publish_command_with_ack(command.message().clone(), Duration::from_secs(2))
        .await?;
    assert!(
        !source.duplicate(),
        "documented nonduplicate command PubAck precondition"
    );
    let republish = f.publisher().publish_command_response(diagnostic).await;
    println!("nonduplicate source publish, repeated capacity diagnostic result={republish:?}");
    assert!(
        republish.is_ok(),
        "command-stream dedup verification must not conceal a response-stream dedup tombstone"
    );
    Ok(())
}

#[tokio::test]
async fn competing_completed_and_technical_writers_cannot_replace_business_evidence() -> TestResult
{
    let f = Fixture::new("review-writer-race").await?;
    let (_, command) = command(&f)?;
    let a = accepted(&command)?;
    let t = diagnostic(&command, StoreFailureKind::Unavailable)?;
    let p1 = f.publisher();
    let p2 = f.publisher();
    let (business, technical) = tokio::join!(
        p1.publish_command_response(a.clone()),
        p2.publish_command_response(t.clone())
    );
    business?;
    println!("competing technical writer={technical:?}");
    assert!(f.publisher().publish_command_response(t).await.is_err());
    assert!(f.publisher().publish_command_response(a).await?.duplicate());
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let response = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(2),
        )
        .await?;
    assert_eq!(response.outcome(), &CommandResponseOutcome::Accepted);
    Ok(())
}

#[test]
fn technical_wire_must_fail_closed_on_contradictory_or_unknown_recovery() {
    let base = serde_json::json!({
        "message_id":"response", "command_message_id":"command", "command_address":"acme.command.orders.place-order",
        "operation_id":"operation", "schema_version":2, "correlation_id":"correlation",
        "outcome":{"status":"execution_failed","value":{"source":"store","kind":"corrupt_history"}}
    });
    let mut refused = Vec::new();
    for recovery in ["retry", "future_recovery"] {
        let mut wire = base.clone();
        wire["outcome"]["value"]["recovery"] = serde_json::json!(recovery);
        let decoded = serde_json::from_value::<CommandResponse>(wire);
        println!("recovery={recovery}, decoded={decoded:?}");
        refused.push(decoded.is_err());
    }
    assert!(
        refused.iter().all(|refused| *refused),
        "undeclared recovery categories must fail closed: {refused:?}"
    );
}

#[tokio::test]
async fn competing_accept_reject_and_diagnostic_writers_preserve_one_terminal_winner() -> TestResult
{
    let f = Fixture::new("review-three-writers").await?;
    let (_, command) = command(&f)?;
    let a = accepted(&command)?;
    let rejection = CommandResponse::rejected(
        rostfrei::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        CommandRejection::new(
            CommandRejectionClassification::NotFound,
            ApplicationErrorCode::new("review.not-found")?,
            "not found",
            None,
        )?,
    )?;
    let r = OutboundMessage::json(
        command.response_address()?,
        rejection.message_id().clone(),
        &rejection,
    )?;
    let t = diagnostic(&command, StoreFailureKind::Unavailable)?;
    let gate = Arc::new(tokio::sync::Barrier::new(3));
    let publish = |message| {
        let gate = gate.clone();
        let publisher = f.publisher();
        async move {
            gate.wait().await;
            publisher.publish_command_response(message).await
        }
    };
    let (accept, reject, technical) =
        tokio::join!(publish(a.clone()), publish(r.clone()), publish(t.clone()));
    println!("three writers accept={accept:?}, reject={reject:?}, technical={technical:?}");
    assert!(
        accept.is_ok() ^ reject.is_ok(),
        "exactly one terminal business payload wins"
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let before = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(2),
        )
        .await?;
    assert!(!matches!(
        before.outcome(),
        CommandResponseOutcome::ExecutionFailed(_)
    ));
    assert!(f.publisher().publish_command_response(t).await.is_err());
    if accept.is_ok() {
        assert!(f.publisher().publish_command_response(r).await.is_err());
    } else {
        assert!(f.publisher().publish_command_response(a).await.is_err());
    }
    let after = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(2),
        )
        .await?;
    assert_eq!(before, after);
    Ok(())
}

struct Proxy(tokio::task::JoinHandle<TestResult>);
impl Drop for Proxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn drop_first_puback_proxy(
    stream_name: String,
) -> TestResult<(Proxy, std::net::SocketAddr, Arc<AtomicUsize>)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream_url = std::env::var("ROSTFREI_NATS_URL")?;
    let upstream_address = upstream_url
        .rsplit('@')
        .next()
        .ok_or("missing fixture address")?
        .to_owned();
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = dropped.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream_address).await?;
        let (mut client_read, mut client_write) = client.into_split();
        let (upstream_read, mut upstream_write) = upstream.into_split();
        let upload = async {
            tokio::io::copy(&mut client_read, &mut upstream_write).await?;
            TestResult::Ok(())
        };
        let download = async {
            let mut reader = BufReader::new(upstream_read);
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
                        .ok_or("missing MSG length")?
                        .parse()?;
                    let mut body = vec![0; length.checked_add(2).ok_or("wire length overflow")?];
                    reader.read_exact(&mut body).await?;
                    let ack = format!("\"stream\":\"{stream_name}\"");
                    if body.windows(ack.len()).any(|w| w == ack.as_bytes())
                        && count.load(Ordering::SeqCst) == 0
                    {
                        count.fetch_add(1, Ordering::SeqCst);
                        // Drop the actual broker PubAck only; connection, request, and stored bytes survive.
                        continue;
                    }
                    client_write.write_all(&line).await?;
                    client_write.write_all(&body).await?;
                } else {
                    client_write.write_all(&line).await?;
                }
            }
        };
        tokio::select! { result=upload => result, result=download => result }
    });
    Ok((Proxy(task), address, dropped))
}

#[tokio::test]
async fn real_lost_puback_retries_exact_stored_response_without_claiming_changed_payload()
-> TestResult {
    let f = Fixture::new("review-lost-puback").await?;
    let (_, command) = command(&f)?;
    let (proxy, address, dropped) = drop_first_puback_proxy(
        f.messaging
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
        f.messaging.topology().clone(),
    );
    let message = diagnostic(&command, StoreFailureKind::Unavailable)?;
    let lost = publisher
        .publish_command_response_with_ack(message.clone(), Duration::from_millis(300))
        .await;
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "real broker PubAck was intercepted"
    );
    assert!(matches!(lost, Err(NatsError::PublishTimeout)));
    let retry = publisher
        .publish_command_response_with_ack(message, Duration::from_secs(2))
        .await?;
    assert!(retry.duplicate());
    let failure = ExecutionFailure::Store(StoreFailureKind::Conflict);
    // Same cached publication ID, different bytes: must not return a fresh/durable success.
    let altered = CommandResponse::execution_failed(
        command_technical_response_message_id(
            command.message_id(),
            ExecutionFailure::Store(StoreFailureKind::Unavailable),
        )?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        failure,
    )?;
    let altered = OutboundMessage::json(
        command.response_address()?,
        altered.message_id().clone(),
        &altered,
    )?;
    assert!(
        publisher
            .publish_command_response_with_ack(altered, Duration::from_secs(2))
            .await
            .is_err()
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let retained = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(2),
        )
        .await?;
    assert_eq!(
        retained.outcome(),
        &CommandResponseOutcome::ExecutionFailed(ExecutionFailure::Store(
            StoreFailureKind::Unavailable
        ))
    );
    client.drain().await?;
    drop(proxy);
    println!(
        "lost real PubAck: exact retry duplicate seq={}, different payload refused",
        retry.sequence()
    );
    Ok(())
}

#[tokio::test]
async fn redrive_validates_all_canonical_dimensions_before_lookup_or_deletion() -> TestResult {
    let f = Fixture::new("remediation-redrive-matrix").await?;
    let (_, original) = command(&f)?;
    f.publisher()
        .publish_command_response(diagnostic(&original, StoreFailureKind::CorruptHistory)?)
        .await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let base: serde_json::Value = serde_json::from_slice(original.payload())?;
    let mutations = [
        (
            "payload",
            serde_json::json!({"command":"act","schema_version":1,"payload":{"target":"changed","action":"noop","item":"I"}}),
        ),
        ("message_id", serde_json::json!("changed-inner-id")),
        ("operation_id", serde_json::json!("changed-operation")),
        ("schema_version", serde_json::json!(2)),
        ("correlation_id", serde_json::json!("changed-correlation")),
        ("causation_id", serde_json::json!("changed-causation")),
        (
            "payload",
            serde_json::json!({"command":"unknown","schema_version":1,"payload":{"target":"target","action":"noop","item":"I"}}),
        ),
        (
            "payload",
            serde_json::json!({"command":"act","schema_version":2,"payload":{"target":"target","action":"noop","item":"I"}}),
        ),
        (
            "payload",
            serde_json::json!({"command":"act","schema_version":1,"events_caused_by_command":true,"payload":{"target":"target","action":"noop","item":"I"}}),
        ),
    ];
    for (field, value) in mutations {
        let mut wire = base.clone();
        *wire.get_mut(field).ok_or("missing fixture dimension")? = value;
        let invalid = EncodedCommand::from_delivery(
            original.address().clone(),
            original.message_id().clone(),
            serde_json::to_vec(&wire)?,
        )?;
        assert!(
            invalid.validate().is_err(),
            "invalid {field} must fail pure validation"
        );
        assert!(
            reader
                .remove_blocked_diagnostic_for_redrive(&invalid, Duration::from_secs(2))
                .await
                .is_err()
        );
        assert!(
            reader
                .find_command_response(
                    &original.response_address()?,
                    original.operation_id(),
                    original.message_id(),
                    Duration::from_secs(2)
                )
                .await?
                .is_some()
        );
    }
    for address in [
        CommandAddress::new(
            original.address().application(),
            original.address().context(),
            "different-route",
        )?,
        CommandAddress::new(original.address().application(), "different-context", "act")?,
        CommandAddress::new("different-application", original.address().context(), "act")?,
        CommandAddress::new_in_scope(
            original.address().application(),
            rostfrei::TrafficScope::Test,
            original.address().context(),
            "act",
        )?,
    ] {
        let invalid = EncodedCommand::from_delivery(
            address,
            original.message_id().clone(),
            original.payload().to_vec(),
        )?;
        assert!(
            reader
                .remove_blocked_diagnostic_for_redrive(&invalid, Duration::from_secs(2))
                .await
                .is_err()
        );
    }
    let invalid = EncodedCommand::from_delivery(
        original.address().clone(),
        MessageId::new("changed-outer-id")?,
        original.payload().to_vec(),
    )?;
    assert!(
        reader
            .remove_blocked_diagnostic_for_redrive(&invalid, Duration::from_secs(2))
            .await
            .is_err()
    );
    // A canonical but foreign-scoped command is not authority to inspect this reader's sink.
    let foreign = local_bus(
        &ApplicationName::new("foreign-application")?.bounded_context("release-contract")?,
        processor(
            &f.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    )
    .encode(request("review-operation", "target", "noop")?)?;
    foreign.validate()?;
    assert!(
        reader
            .remove_blocked_diagnostic_for_redrive(&foreign, Duration::from_secs(2))
            .await
            .is_err()
    );
    assert!(
        reader
            .find_command_response(
                &original.response_address()?,
                original.operation_id(),
                original.message_id(),
                Duration::from_secs(2)
            )
            .await?
            .is_some()
    );
    f.js.delete_stream(f.messaging.topology().command_response_stream().as_str())
        .await?;
    assert_eq!(
        reader
            .remove_blocked_diagnostic_for_redrive(&invalid, Duration::from_secs(2))
            .await
            .unwrap_err()
            .kind(),
        CommandResponseReadErrorKind::IdentityConflict,
        "invalid evidence must be refused before even looking up an unavailable sink"
    );
    Ok(())
}

#[test]
fn technical_extensions_are_strict_but_legacy_business_extensions_remain_compatible() {
    let technical = serde_json::json!({
        "message_id":"response", "command_message_id":"command", "command_address":"acme.command.orders.place-order",
        "operation_id":"operation", "schema_version":2, "correlation_id":"correlation",
        "outcome":{"status":"execution_failed","value":{"source":"store","kind":"corrupt_history"}}
    });
    assert!(serde_json::from_value::<CommandResponse>(technical.clone()).is_ok());
    let mut extra = technical.clone();
    extra
        .as_object_mut()
        .unwrap()
        .insert("recovery".to_owned(), serde_json::json!("retry"));
    assert!(serde_json::from_value::<CommandResponse>(extra).is_err());
    let mut extra = technical.clone();
    extra
        .get_mut("outcome")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("recovery".to_owned(), serde_json::json!("future_recovery"));
    assert!(serde_json::from_value::<CommandResponse>(extra).is_err());
    let mut legacy = technical;
    *legacy.get_mut("schema_version").unwrap() = serde_json::json!(1);
    *legacy.get_mut("outcome").unwrap() =
        serde_json::json!({"status":"accepted", "legacy_extension":true});
    legacy
        .as_object_mut()
        .unwrap()
        .insert("legacy_extension".to_owned(), serde_json::json!(true));
    assert!(serde_json::from_value::<CommandResponse>(legacy).is_ok());
}

rostfrei::install_macro_support!();
