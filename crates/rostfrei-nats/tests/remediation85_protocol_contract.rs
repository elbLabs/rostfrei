//! Raw-wire compatibility and native disposition controls for protocol remediation R3/R4/R5.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use rostfrei::EncodedCommand;

const PREFIX: &str = r#""message_id":"response","command_message_id":"command","command_address":"acme.command.orders.place-order","operation_id":"operation","correlation_id":"correlation""#;

fn wire(version: u32, outcome: &str) -> String {
    format!("{{{PREFIX},\"schema_version\":{version},\"outcome\":{outcome}}}")
}

#[test]
fn raw_duplicate_matrix_preserves_typed_checks_in_both_field_orders_and_versions() {
    let accepted = wire(1, r#"{"status":"accepted"}"#);
    let rejected = wire(
        1,
        r#"{"status":"rejected","value":{"classification":"not_found","code":"test.missing","message":"missing","details":null}}"#,
    );
    let technical = wire(
        2,
        r#"{"status":"execution_failed","value":{"source":"store","kind":"unavailable"}}"#,
    );
    let content_first = wire(
        2,
        r#"{"value":{"kind":"unavailable","source":"store"},"status":"execution_failed"}"#,
    );
    for base in [&accepted, &rejected, &technical, &content_first] {
        assert!(serde_json::from_str::<CommandResponse>(base).is_ok());
        for field in [
            r#""message_id":"response""#,
            r#""command_message_id":"command""#,
            r#""command_address":"acme.command.orders.place-order""#,
            r#""operation_id":"operation""#,
            r#""correlation_id":"correlation""#,
            if base == &accepted || base == &rejected {
                r#""schema_version":1"#
            } else {
                r#""schema_version":2"#
            },
        ] {
            let raw = base.replacen(field, &format!("{field},{field}"), 1);
            assert_ne!(&raw, base);
            assert!(
                serde_json::from_str::<CommandResponse>(&raw).is_err(),
                "duplicate known {field}: {raw}"
            );
        }
        // JSON escape spellings still identify the SAME known field.
        let raw = base.replacen(
            r#""message_id":"response""#,
            r#""message_id":"response","\u006dessage_id":"response""#,
            1,
        );
        assert!(serde_json::from_str::<CommandResponse>(&raw).is_err());
        let raw = base.replacen(
            r#""outcome":"#,
            r#""outcome":{"status":"accepted"},"outcome":"#,
            1,
        );
        assert!(serde_json::from_str::<CommandResponse>(&raw).is_err());
    }
    for (base, field) in [
        (&accepted, r#""status":"accepted""#),
        (&rejected, r#""status":"rejected""#),
        (&technical, r#""status":"execution_failed""#),
        (&content_first, r#""status":"execution_failed""#),
        (&technical, r#""source":"store""#),
        (&content_first, r#""source":"store""#),
        (&technical, r#""kind":"unavailable""#),
        (&content_first, r#""kind":"unavailable""#),
        (&rejected, r#""classification":"not_found""#),
        (&rejected, r#""code":"test.missing""#),
        (&rejected, r#""message":"missing""#),
        (&rejected, r#""details":null"#),
    ] {
        let raw = base.replacen(field, &format!("{field},{field}"), 1);
        assert_ne!(&raw, base);
        assert!(
            serde_json::from_str::<CommandResponse>(&raw).is_err(),
            "duplicate typed field: {raw}"
        );
    }
    for outcome in [
        r#"{"value":{"source":"store","kind":"unavailable"},"value":{"source":"store","kind":"unavailable"},"status":"execution_failed"}"#,
        r#"{"status":"execution_failed","value":{"source":"store","kind":"unavailable"},"value":{"source":"store","kind":"unavailable"}}"#,
    ] {
        assert!(serde_json::from_str::<CommandResponse>(&wire(2, outcome)).is_err());
    }
}

#[test]
fn raw_unknown_legacy_extensions_remain_distinct_from_strict_technical_extensions() {
    let legacy = format!(
        r#"{{"future":{{"message_id":"ignored","message_id":"also-ignored"}},{PREFIX},"schema_version":1,"outcome":{{"future":true,"status":"accepted","future":false}},"future":null}}"#
    );
    assert_eq!(
        serde_json::from_str::<CommandResponse>(&legacy)
            .unwrap()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    for outcome in [
        r#"{"extra":true,"status":"execution_failed","value":{"source":"store","kind":"unavailable"}}"#,
        r#"{"value":{"recovery":"retry","kind":"unavailable","source":"store"},"status":"execution_failed"}"#,
        r#"{"value":{"kind":"unavailable","source":"store"},"status":"execution_failed","recovery":"retry"}"#,
    ] {
        assert!(serde_json::from_str::<CommandResponse>(&wire(2, outcome)).is_err());
    }
    let raw = format!(
        r#"{{"future":true,{PREFIX},"schema_version":2,"outcome":{{"status":"execution_failed","value":{{"kind":"unavailable","source":"store"}}}}}}"#
    );
    assert!(serde_json::from_str::<CommandResponse>(&raw).is_err());
}

fn encoded(f: &Fixture, operation: &str, target: &str) -> TestResult<EncodedCommand> {
    local_bus(
        &f.context,
        processor(
            &f.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    )
    .encode(request(operation, target, "create")?)
    .map_err(Into::into)
}

fn diagnostic(c: &EncodedCommand) -> TestResult<OutboundMessage<CommandResponseAddress>> {
    let failure = ExecutionFailure::Store(StoreFailureKind::Unavailable);
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

#[tokio::test]
async fn ambiguous_native_bytes_are_refused_before_write_and_by_live_reader_if_injected()
-> TestResult {
    use sha2::{Digest, Sha256};
    let f = Fixture::new("protocol-raw-refusal").await?;
    let c = encoded(&f, "operation", "target")?;
    let valid = diagnostic(&c)?;
    let ambiguous = std::str::from_utf8(valid.payload())?
        .replacen(
            "\"kind\":\"unavailable\"",
            "\"kind\":\"corrupt_history\",\"kind\":\"unavailable\"",
            1,
        )
        .into_bytes();
    assert_ne!(ambiguous, valid.payload());
    let outbound = OutboundMessage::new(
        c.response_address()?,
        valid.message_id().clone(),
        ambiguous.clone(),
    )?;
    assert!(
        f.publisher()
            .prepare_command_response(outbound.clone(), Duration::from_secs(2))
            .await
            .is_err()
    );
    assert!(
        f.publisher()
            .publish_command_response(outbound)
            .await
            .is_err()
    );
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
            .is_none()
    );
    let stream =
        f.js.get_stream(f.messaging.topology().command_response_stream().as_str())
            .await?;
    assert_eq!(
        stream.cached_info().state.messages,
        0,
        "invalid preparation/publication must have no durable write"
    );
    let prepared = f
        .publisher()
        .prepare_command_response(valid, Duration::from_secs(2))
        .await?;
    let token = serde_json::to_vec(prepared.transition().ok_or("missing token")?)?;
    let address = c.response_address()?.to_string();
    let mut hash = Sha256::new();
    for frame in [
        b"rostfrei:diagnostic-publication:v1".as_slice(),
        address.as_bytes(),
        &ambiguous,
        &token,
    ] {
        hash.update(u64::try_from(frame.len())?.to_be_bytes());
        hash.update(frame);
    }
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert(CORRELATION_ID_HEADER, c.correlation_id().as_str());
    headers.insert(DIAGNOSTIC_TRANSITION_HEADER, String::from_utf8(token)?);
    headers.insert("Nats-Msg-Id", format!("{:x}", hash.finalize()));
    f.js.publish_with_headers(address, headers, ambiguous.clone().into())
        .await?
        .await?;
    assert_eq!(
        reader
            .find_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .kind(),
        CommandResponseReadErrorKind::InvalidResponse
    );
    let retained = stream
        .get_last_raw_message_by_subject(c.response_address()?.as_str())
        .await?;
    assert_eq!(
        retained.payload.as_ref(),
        ambiguous,
        "unknown/ambiguous evidence must remain for audit, not be deleted"
    );
    Ok(())
}

#[tokio::test]
async fn legacy_business_reader_still_allows_absent_outer_correlation() -> TestResult {
    let f = Fixture::new("protocol-legacy-optional").await?;
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    for header_present in [true, false] {
        let c = encoded(
            &f,
            if header_present { "present" } else { "absent" },
            "target",
        )?;
        let response = CommandResponse::accepted(
            rostfrei::command_response_message_id(c.message_id())?,
            c.message_id().clone(),
            c.address().clone(),
            c.operation_id().clone(),
            c.correlation_id().clone(),
        )?;
        let payload = serde_json::to_vec(&response)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert("Nats-Msg-Id", response.message_id().as_str());
        if header_present {
            headers.insert(CORRELATION_ID_HEADER, c.correlation_id().as_str());
        }
        f.js.publish_with_headers(c.response_address()?.to_string(), headers, payload.into())
            .await?
            .await?;
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
    }
    Ok(())
}

rostfrei::install_macro_support!();
