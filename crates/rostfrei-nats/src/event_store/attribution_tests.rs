use super::*;

fn actor(id: &str) -> rostfrei_core::ActorRef {
    rostfrei_core::ActorRef::new("people", id).unwrap()
}

fn fixture() -> (NatsEventStoreConfig, StreamId, EventBatch) {
    let context = rostfrei_messaging_core::ApplicationName::new("acme")
        .unwrap()
        .bounded_context_in_scope(rostfrei_messaging_core::TrafficScope::Test, "orders")
        .unwrap();
    let config = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
    let stream = StreamId::new(
        AggregateType::new("Test").unwrap(),
        AggregateId::new("one").unwrap(),
    );
    let op = OperationId::new("attribution").unwrap();
    let commit = derive_commit_id(&stream, &op);
    let batch = EventBatch::new(
        commit.clone(),
        op,
        ContentFingerprint::digest("same"),
        (0..2)
            .map(|i| {
                NewEvent::new(derive_event_id(&commit, i), "opened", 1, b"{}".to_vec()).unwrap()
            })
            .collect(),
    )
    .unwrap()
    .with_actor(actor("alice"));
    (config, stream, batch)
}

fn headers(config: &NatsEventStoreConfig, ordinal: usize) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert(NATS_BATCH_ID, "attribution-batch");
    headers.insert(
        NATS_BATCH_SEQUENCE,
        ordinal.checked_add(1).unwrap().to_string(),
    );
    if ordinal == 0 {
        headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
        headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
        headers.insert(
            NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT,
            config.aggregate_history_subject_filter("Test", "one"),
        );
    }
    headers
}

#[test]
#[allow(clippy::too_many_lines)]
fn reconstruction_refuses_internally_checksummed_actor_disagreement() {
    let (config, stream, batch) = fixture();
    let recorded = record_batch(&stream, StreamVersion::ZERO, &batch).unwrap();
    let bytes =
        encode_transaction_events(&config, &stream, &batch, recorded.events(), 0, 2).unwrap();
    let subject = config.event_subject(
        stream.aggregate_type().as_str(),
        stream.aggregate_id().as_str(),
    );
    for changed in [false, true] {
        let mut builder = HistoryBuilder::default();
        for (i, bytes) in bytes.iter().enumerate() {
            let mut wire: StoredEventWire = serde_json::from_slice(bytes).unwrap();
            if changed && i == 1 {
                wire.event.actor = Some(actor("bob"));
                wire.checksum = event_checksum(wire.schema_version, &wire.event).unwrap();
            }
            let bytes = serde_json::to_vec(&wire).unwrap();
            let decoded = decode_event(
                &config,
                &subject,
                &stream,
                Some(0),
                u64::try_from(i + 1).unwrap(),
                &headers(&config, i),
                &bytes,
            )
            .unwrap();
            if changed && i == 1 {
                assert_eq!(
                    builder.push(decoded).unwrap_err().kind(),
                    EventStoreErrorKind::CorruptHistory
                );
                break;
            }
            builder.push(decoded).unwrap();
        }
        if changed {
            continue;
        }
        let history = builder.finish(2).unwrap();
        let participant = TransactionParticipantWire {
            stream: stream_identity(&stream),
            base_stream_version: 0,
            commit_id: Some(batch.commit_id().as_str().to_owned()),
            event_count: 2,
        };
        for actor in [None, Some(actor("bob")), Some(actor("alice"))] {
            let result = materialize_transaction_participant(
                &history,
                &participant,
                StreamVersion::ZERO,
                batch.operation_id(),
                batch.operation_fingerprint(),
                None,
                None,
                actor.as_ref(),
                "attribution-batch",
                0,
                2,
                1,
            );
            if actor.as_ref() == batch.actor() {
                assert_eq!(result.unwrap(), recorded.events());
            } else {
                assert_eq!(
                    result.unwrap_err().kind(),
                    EventStoreErrorKind::CorruptHistory
                );
            }
        }
        let receipt = TransactionReceipt::new(
            batch.operation_id().clone(),
            batch.operation_fingerprint(),
            vec![TransactionStreamReceipt::new(
                stream.clone(),
                StreamVersion::ZERO,
                history.events,
            )],
        )
        .with_bounded_context(config.bounded_context().clone())
        .with_actor(actor("alice"));
        let proposal = outcomes::proposal_from_receipt(&receipt).unwrap();
        assert_eq!(proposal.actor(), batch.actor());
        assert_eq!(
            proposal.participants().first().unwrap().batch(),
            Some(&batch)
        );
        let (marker, details) = rostfrei_core::OutcomeMarker::prepare(
            rostfrei_core::CommandAuthority::new("owned").unwrap(),
            &proposal,
            receipt.clone(),
        )
        .unwrap();
        let restored = rostfrei_core::OutcomeMarker::restore_manifest(
            marker.detail_reference().key(),
            &marker.encode_manifest().unwrap(),
            receipt,
        )
        .unwrap();
        assert_eq!(restored, marker);
        let details =
            rostfrei_core::EncodedOutcomeDetails::decode(restored.detail_reference(), &details)
                .unwrap();
        assert_eq!(details.detail_reference().actor(), batch.actor());
    }
}

#[test]
fn event_discriminators_reject_actor_in_legacy_and_absence_in_new_even_with_new_checksums() {
    let (config, stream, batch) = fixture();
    let recorded = record_batch(&stream, StreamVersion::ZERO, &batch).unwrap();
    let encoded =
        encode_transaction_events(&config, &stream, &batch, recorded.events(), 0, 2).unwrap();
    let template: StoredEventWire = serde_json::from_slice(encoded.first().unwrap()).unwrap();
    let subject = config.event_subject(
        stream.aggregate_type().as_str(),
        stream.aggregate_id().as_str(),
    );
    for version in 1..=8 {
        let mut base = template.clone();
        base.schema_version = version;
        let attributed = matches!(version, 5 | 6 | 8);
        if !attributed {
            base.event.actor = None;
        }
        if version < 3 {
            base.event.application = None;
            base.event.bounded_context = None;
        }
        if !transactional_event_schema(version) {
            base.event.transaction_event_ordinal = None;
            base.event.transaction_event_count = None;
        }
        let check = |wire: &StoredEventWire| {
            let mut wire = wire.clone();
            wire.checksum = event_checksum(wire.schema_version, &wire.event).unwrap();
            decode_consumed_event(
                &config,
                &subject,
                &headers(&config, 0),
                &serde_json::to_vec(&wire).unwrap(),
            )
        };
        check(&base).unwrap();
        let mut changed = base.clone();
        changed.event.actor = if attributed {
            None
        } else {
            Some(actor("alice"))
        };
        assert!(
            matches!(check(&changed), Err(e) if e.kind() == EventStoreErrorKind::CorruptHistory)
        );
        if matches!(version, 6 | 8) {
            base.checksum = event_checksum(version, &base.event).unwrap();
            rejects_actor_mutations(&serde_json::to_vec(&base).unwrap(), "event", |bytes| {
                decode_consumed_event(&config, &subject, &headers(&config, 0), bytes).map(|_| ())
            });
        }
        let mut null = serde_json::to_value(base).unwrap();
        null["event"]["actor"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<StoredEventWire>(null).is_err());
    }
}

fn rejects_actor_mutations(
    bytes: &[u8],
    field: &str,
    decode: impl Fn(&[u8]) -> Result<(), EventStoreError>,
) {
    decode(bytes).unwrap();
    let wire: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    for mutation in ["replace", "strip", "null", "legacy-version"] {
        let mut bad = wire.clone();
        match mutation {
            "replace" => bad[field]["actor"] = serde_json::to_value(actor("bob")).unwrap(),
            "strip" => {
                bad[field].as_object_mut().unwrap().remove("actor");
            }
            "null" => bad[field]["actor"] = serde_json::Value::Null,
            _ => bad["schemaVersion"] = serde_json::json!(4),
        }
        assert_eq!(
            decode(&serde_json::to_vec(&bad).unwrap())
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::CorruptHistory,
            "{field}: {mutation}"
        );
    }
}

#[test]
fn receipt_attribution_rejects_rechecksummed_downgrades_and_duplicate_claims() {
    let (config, _, _) = fixture();
    let mut headers = headers(&config, 0);
    headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
    headers.insert(
        NATS_EXPECTED_LAST_SUBJECT_SEQUENCE_SUBJECT,
        config.transaction_receipt_subject_filter("receipt-attribution"),
    );
    let template = TransactionReceiptContentWire {
        event_store_stream: config.stream_name().to_owned(),
        application: config.application().as_str().to_owned(),
        bounded_context: config.bounded_context().as_str().to_owned(),
        operation_id: "receipt-attribution".to_owned(),
        operation_fingerprint: ContentFingerprint::digest("receipt-attribution").to_hex(),
        correlation_id: None,
        causation_id: None,
        security_scope: None,
        actor: Some(actor("alice")),
        completion: None,
        manifest: None,
        participants: vec![],
    };
    let valid = encode_transaction_receipt(&template).unwrap();
    rejects_actor_mutations(&valid, "receipt", |bytes| {
        decode_transaction_receipt(&config, 1, &headers, bytes).map(|_| ())
    });
    // Fresh checksums ensure version dispatch, rather than checksum rejection, is tested.
    for (schema_version, present, accepted) in [
        (2, false, true),
        (5, true, true),
        (2, true, false),
        (5, false, false),
        (3, false, false), // Inline completion is required by this schema.
        (4, false, false), // Split completion requires its manifest.
        (u16::MAX, true, false),
    ] {
        let mut receipt = template.clone();
        receipt.actor = present.then(|| actor("alice"));
        let wire = StoredTransactionReceiptWire {
            checksum: transaction_receipt_checksum(schema_version, &receipt).unwrap(),
            schema_version,
            receipt,
        };
        let result =
            decode_transaction_receipt(&config, 1, &headers, &serde_json::to_vec(&wire).unwrap());
        if accepted {
            result.unwrap();
        } else {
            assert_eq!(
                result.err().unwrap().kind(),
                EventStoreErrorKind::CorruptHistory
            );
        }
    }
    let valid = String::from_utf8(valid).unwrap();
    for replacement in [
        r#""actor":null,"actor":{"namespace":"people","id":"alice"}"#,
        r#""actor":{"namespace":"people","id":"alice"},"actor":{"namespace":"people","id":"alice"}"#,
        r#""actor":{"namespace":"people","namespace":"people","id":"alice"}"#,
        r#""actor":{"namespace":"people","id":"alice","extra":0}"#,
    ] {
        let bad = valid.replace(
            r#""actor":{"namespace":"people","id":"alice"}"#,
            replacement,
        );
        assert_ne!(bad, valid);
        assert_eq!(
            decode_transaction_receipt(&config, 1, &headers, bad.as_bytes())
                .err()
                .unwrap()
                .kind(),
            EventStoreErrorKind::CorruptHistory
        );
    }
}
