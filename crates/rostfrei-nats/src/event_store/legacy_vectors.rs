//! Byte vectors captured from the pre-actor 48034e0 source, not regenerated expectations.
use super::*;

#[allow(
    clippy::too_many_lines,
    reason = "fixed legacy fixture covers all serialized boundaries together"
)]
fn serialized_vectors() -> serde_json::Value {
    use rostfrei_core::{
        CommandAuthority, CommandCompletion, CommandExecutionMetadata, OutcomeMarker,
        ProposalDigest,
    };
    use rostfrei_messaging_core::{
        ApplicationName, CommandEnvelope, EnvelopeContext, IntegrationEventEnvelope, MessageId,
        MessageTimestamp, SchemaVersion,
    };
    let context = ApplicationName::new("legacy-vectors")
        .unwrap()
        .bounded_context("orders")
        .unwrap();
    let config = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
    let op = OperationId::new("vector-operation").unwrap();
    let stream = StreamId::new(
        AggregateType::new("order").unwrap(),
        AggregateId::new("one").unwrap(),
    );
    let commit = derive_commit_id(&stream, &op);
    let fingerprint = ContentFingerprint::digest("legacy content");
    let correlation = CorrelationId::new("vector-correlation").unwrap();
    let causation = CausationId::new("vector-causation").unwrap();
    let batch = EventBatch::new(
        commit.clone(),
        op.clone(),
        fingerprint,
        vec![
            NewEvent::new(
                derive_event_id(&commit, 0),
                "opened",
                1,
                br#"{"id":7}"#.to_vec(),
            )
            .unwrap(),
        ],
    )
    .unwrap()
    .with_correlation_id(correlation.clone())
    .with_causation_id(causation.clone());
    let events = record_batch(&stream, StreamVersion::ZERO, &batch).unwrap();
    let envelope_context = EnvelopeContext::new(
        MessageId::new("vector-message").unwrap(),
        SchemaVersion::new(1).unwrap(),
        correlation.clone(),
        Some(causation.clone()),
    );
    let timestamp = MessageTimestamp::from_unix_milliseconds(42).unwrap();
    let command = CommandEnvelope::new(
        envelope_context.clone(),
        rostfrei_messaging_core::OperationId::new(op.as_str()).unwrap(),
        timestamp,
        serde_json::json!({"domain":1}),
    )
    .unwrap();
    let integration =
        IntegrationEventEnvelope::new(envelope_context, timestamp, serde_json::json!({"domain":1}))
            .unwrap();
    let processor = Arc::new(rostfrei::CommandProcessor::new(
        context.name().clone(),
        Arc::new(rostfrei_core::InMemoryEventStore::new()),
    ));
    let bus = rostfrei::CommandBus::new(
        context.clone(),
        Arc::new(rostfrei::InMemoryMessagingAdapter::new(processor)),
    );
    let encoded = bus
        .encode_dynamic(
            rostfrei::DynamicCommandRequest::new(
                op.clone(),
                "open",
                1,
                serde_json::json!({"id":7}),
            )
            .unwrap()
            .with_correlation_id(correlation.clone())
            .with_causation_id(causation.clone())
            .with_created_at(timestamp),
        )
        .unwrap();
    let metadata = CommandExecutionMetadata::new(op.clone(), fingerprint)
        .with_default_outcome_evidence::<()>();
    let mut rows = Vec::new();
    for shape in ["raw-write", "raw-guard", "accepted", "noevent", "rejected"] {
        let write = matches!(shape, "raw-write" | "accepted");
        let mut proposal = EventTransaction::new(
            op.clone(),
            fingerprint,
            vec![TransactionParticipant::new(
                stream.clone(),
                ExpectedVersion::NoStream,
                write.then(|| batch.clone()),
            )],
        )
        .with_bounded_context(context.name().clone())
        .with_correlation_id(correlation.clone())
        .with_causation_id(causation.clone());
        if !shape.starts_with("raw") {
            let rejection = if shape == "rejected" {
                Some(rostfrei_core::EncodedRejection {
                    codec_id: "reason".to_owned(),
                    codec_version: 1,
                    bytes: b"reason-bytes".to_vec(),
                })
            } else {
                None
            };
            proposal = proposal.with_completion(CommandCompletion::new(
                metadata.outcome_evidence().cloned(),
                rejection,
            ));
        }
        let staged = vec![StagedTransactionParticipant {
            stream_id: stream.clone(),
            base_version: StreamVersion::ZERO,
            last_subject_stream_sequence: 0,
            batch: write.then(|| batch.clone()),
            recorded: if write {
                events.events().to_vec()
            } else {
                Vec::new()
            },
        }];
        let mut receipt = outcomes::staged_receipt(&proposal, &staged);
        if let Some(completion) = proposal.completion() {
            receipt = receipt.with_completion(completion.clone());
        }
        let (marker, details) = OutcomeMarker::prepare(
            CommandAuthority::new("vector-authority").unwrap(),
            &proposal,
            receipt,
        )
        .unwrap();
        let mut receipt_wire = transaction_receipt_content(&config, &proposal, &staged).unwrap();
        let inline = String::from_utf8(encode_transaction_receipt(&receipt_wire).unwrap()).unwrap();
        receipt_wire.completion = None;
        receipt_wire.manifest = Some(STANDARD.encode(marker.encode_manifest().unwrap()));
        rows.push(serde_json::json!({
            "shape":shape,
            "proposal":ProposalDigest::of(&proposal).unwrap().fingerprint().to_hex(),
            "manifest":String::from_utf8(marker.encode_manifest().unwrap()).unwrap(),
            "reference":String::from_utf8(marker.detail_reference().encode().unwrap()).unwrap(),
            "details":STANDARD.encode(details),
            "inline_receipt":inline,
            "split_receipt":String::from_utf8(encode_transaction_receipt(&receipt_wire).unwrap()).unwrap(),
        }));
    }
    serde_json::json!({
        "command_envelope":serde_json::to_string(&command).unwrap(),
        "integration_envelope":serde_json::to_string(&integration).unwrap(),
        "encoded_command":String::from_utf8(encoded.payload().to_vec()).unwrap(),
        "command_fingerprint":encoded.fingerprint().to_hex(),
        "standalone_event":String::from_utf8(encode_events(&config, &stream, &batch, events.events()).unwrap().remove(0)).unwrap(),
        "transactional_event":String::from_utf8(encode_transaction_events(&config, &stream, &batch, events.events(), 0, 1).unwrap().remove(0)).unwrap(),
        "outcomes":rows,
    })
}

#[test]
fn actorless_serialization_matches_pre_actor_byte_vectors() {
    let mut vectors = serialized_vectors();
    fingerprint_strings(&mut vectors);
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("legacy_vectors.json")).unwrap();
    assert_eq!(vectors, expected);
}

fn fingerprint_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(bytes) => {
            *bytes = ContentFingerprint::digest(bytes.as_bytes()).to_hex();
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(fingerprint_strings),
        serde_json::Value::Object(values) => values.values_mut().for_each(fingerprint_strings),
        _ => panic!("vector contains a non-string leaf"),
    }
}
