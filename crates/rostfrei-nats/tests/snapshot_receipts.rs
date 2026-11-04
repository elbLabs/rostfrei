//! Receipt-backed snapshot replay uses the permanent transaction authority.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
rostfrei::install_macro_support!();
mod release_contract_support;
use release_contract_support::*;

fn config(fixture: &Fixture) -> TestResult<NatsEventStoreConfig> {
    Ok(NatsEventStoreConfig::for_bounded_context(&fixture.context)?
        .with_fresh_split_outcome_epoch("receipt-snapshot-contract")?
        .with_receipt_backed_history()?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?)
}

fn batch(id: &StreamId, operation: &str, item: &str) -> TestResult<EventBatch> {
    let operation = OperationId::new(operation)?;
    let commit = derive_commit_id(id, &operation);
    Ok(EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest(item),
        vec![<JsonEventCodec as EventCodec<Target>>::encode(
            &JsonEventCodec,
            &Changed {
                created: true,
                item: Some(item.to_owned()),
            },
            derive_event_id(&commit, 0),
        )?],
    )?
    .with_correlation_id(CorrelationId::new("receipt-correlation")?)
    .with_causation_id(CausationId::new("receipt-causation")?)
    .with_actor(ActorRef::new("test", "receipt-actor")?))
}

async fn store(fixture: &Fixture) -> TestResult<NatsEventStore> {
    let config = config(fixture)?;
    provision_event_store(&fixture.js, &config).await?;
    Ok(NatsEventStore::connect(fixture.js.clone(), config)
        .await?
        .with_snapshotting(true))
}

#[tokio::test]
async fn low_level_appends_and_sessions_share_permanent_transaction_receipts() -> TestResult {
    let fixture = Fixture::new("snapshot-receipt-appends").await?;
    let store = store(&fixture).await?;
    let full_reader = store.clone().with_snapshotting(false);
    let id = stream("one")?;
    let first = batch(&id, "first", "one")?;
    let (left, right) = tokio::join!(
        full_reader.append(&id, ExpectedVersion::NoStream, first.clone()),
        store.append(&id, ExpectedVersion::NoStream, first.clone())
    );
    let (left, right) = (left?, right?);
    assert_ne!(left.is_exact_replay(), right.is_exact_replay());
    assert_eq!(left.events(), right.events());
    let receipt = store
        .load_transaction_receipt(first.operation_id())
        .await?
        .ok_or("low-level append did not persist an operation receipt")?;
    assert_eq!(receipt.events(), left.events());
    assert_eq!(receipt.actor(), first.actor());
    assert_eq!(receipt.correlation_id(), first.correlation_id());
    assert_eq!(receipt.causation_id(), first.causation_id());
    assert!(receipt.completion().is_none());

    let second = batch(&id, "second", "two")?;
    let result = full_reader
        .append_session()
        .await?
        .append(
            &id,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            second.clone(),
        )
        .await?;
    assert_eq!(
        store
            .load_transaction_receipt(second.operation_id())
            .await?
            .ok_or("append session did not persist an operation receipt")?
            .events(),
        result.events()
    );
    let replay = store
        .append_session()
        .await?
        .append(&id, ExpectedVersion::NoStream, second)
        .await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.events(), result.events());
    let error = store
        .append(
            &id,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            batch(&id, "first", "changed")?,
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
    assert_eq!(store.audit_history(&id).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn receipt_profile_is_fresh_and_cannot_be_enabled_by_policy_update() -> TestResult {
    let fixture = Fixture::new("snapshot-receipt-profile").await?;
    let legacy = fixture.split_event_store().await?;
    let id = stream("one")?;
    legacy
        .append(&id, ExpectedVersion::NoStream, batch(&id, "old", "old")?)
        .await?;
    for config in [
        config(&fixture)?,
        NatsEventStoreConfig::for_bounded_context(&fixture.context)?
            .with_receipt_backed_history()?,
    ] {
        assert_ne!(config.subject_prefix(), legacy.config().subject_prefix());
        // Neither inline nor split authorities may adopt an unqualified namespace.
        let mut unqualified = config.stream_config();
        unqualified.metadata.remove("rostfrei.history.receipts");
        let original = fixture.js.create_stream(unqualified).await?;
        for result in [
            provision_event_store(&fixture.js, &config).await,
            update_event_store(&fixture.js, &config).await,
            NatsEventStore::connect(fixture.js.clone(), config.clone())
                .await
                .map(|_| ()),
        ] {
            assert_eq!(
                result.unwrap_err().kind(),
                EventStoreErrorKind::ConfigurationMismatch
            );
        }
        assert_eq!(
            fixture
                .js
                .get_stream(config.stream_name())
                .await?
                .cached_info()
                .config
                .metadata,
            original.cached_info().config.metadata
        );
    }
    assert_eq!(legacy.load(&id).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn receipt_profile_refuses_writes_after_its_guarantee_is_removed() -> TestResult {
    let fixture = Fixture::new("snapshot-receipt-downgrade").await?;
    let store = store(&fixture).await?;
    let id = stream("one")?;
    let pending = store.append_session().await?;
    let stream = fixture.js.get_stream(store.config().stream_name()).await?;
    let mut changed = stream.cached_info().config.clone();
    changed.metadata.remove("rostfrei.history.receipts");
    fixture.js.update_stream(changed).await?;
    for result in [
        store
            .append(&id, ExpectedVersion::NoStream, batch(&id, "direct", "one")?)
            .await,
        pending
            .append(
                &id,
                ExpectedVersion::NoStream,
                batch(&id, "session", "two")?,
            )
            .await,
    ] {
        assert_eq!(
            result.unwrap_err().kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
    }
    assert_eq!(fixture.messages(store.config().stream_name()).await?, 0);
    Ok(())
}

#[tokio::test]
async fn accelerated_tail_requires_its_actual_operation_receipt() -> TestResult {
    let fixture = Fixture::new("snapshot-receipt-tail").await?;
    let store = Arc::new(store(&fixture).await?);
    let id = stream("one")?;
    for (number, operation) in [(1, "first"), (2, "second")] {
        let expected = if number == 1 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(number - 1))
        };
        let events = batch(&id, operation, operation)?;
        store
            .append_transaction(
                EventTransaction::new(
                    events.operation_id().clone(),
                    events.operation_fingerprint(),
                    vec![TransactionParticipant::new(
                        id.clone(),
                        expected,
                        Some(events),
                    )],
                )
                .with_actor(ActorRef::new("test", "receipt-actor")?)
                .with_correlation_id(CorrelationId::new("receipt-correlation")?)
                .with_causation_id(CausationId::new("receipt-causation")?),
            )
            .await?;
    }
    let config = NatsReadModelConfig::new(&fixture.context, "snapshot-cache")?;
    provision_read_model(&fixture.js, &config).await?;
    let mut codecs = AggregateCodecs::default().with_snapshot_store(Arc::new(
        NatsSnapshotStore::connect(fixture.js.clone(), config).await?,
    ));
    codecs.register_json_snapshot::<Target>("v1", 1, SnapshotPolicy::Manual)?;
    let executor = CommandExecutor::with_codecs(store.clone(), Arc::new(codecs));
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(1))
        .await?;
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        2
    );

    // Same genuine receipt bytes and operation ID at a different native position
    // must not certify the retained transaction. This bypasses the supported writer.
    let journal = fixture.js.get_stream(store.config().stream_name()).await?;
    let receipt = journal
        .get_last_raw_message_by_subject(&store.config().transaction_subject("second"))
        .await?;
    fixture
        .js
        .publish(receipt.subject, receipt.payload)
        .await?
        .await?;
    let error = executor
        .rehydrate::<Target>(&id)
        .await
        .err()
        .ok_or("snapshot replay accepted a misbound receipt")?;
    assert!(
        matches!(error, SimulationError::Store(ref error) if error.kind() == EventStoreErrorKind::CorruptHistory)
    );
    Ok(())
}

#[tokio::test]
async fn earlier_receipt_cannot_certify_a_repeated_operation_in_the_tail() -> TestResult {
    let fixture = Fixture::new("snapshot-prefix-receipt").await?;
    let store = store(&fixture).await?;
    let id = stream("one")?;
    store
        .append(&id, ExpectedVersion::NoStream, batch(&id, "first", "one")?)
        .await?;
    let checkpoint = store
        .load_replay(&id, None, None)
        .await?
        .checkpoint
        .ok_or("checkpoint")?;
    let journal = fixture.js.get_stream(store.config().stream_name()).await?;
    let original = journal
        .get_last_raw_message_by_subject(
            &store
                .config()
                .aggregate_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str()),
        )
        .await?;
    // Preserve canonical wire field order when checksumming a genuine event with
    // the same identities at the next logical version and a different native slot.
    let wire: serde_json::Value = serde_json::from_slice(&original.payload)?;
    let schema = wire
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or("schema")?;
    let text = std::str::from_utf8(&original.payload)?;
    let event = text
        .split_once("\"event\":")
        .ok_or("event")?
        .1
        .strip_suffix('}')
        .ok_or("wire envelope")?
        .replace("\"streamVersion\":1,", "\"streamVersion\":2,");
    let checksum =
        ContentFingerprint::digest(format!("{{\"schemaVersion\":{schema},\"event\":{event}}}"))
            .to_hex();
    let payload =
        format!("{{\"schemaVersion\":{schema},\"checksum\":\"{checksum}\",\"event\":{event}}}");
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert("Nats-Required-Api-Level", "2");
    headers.insert("Nats-Expected-Stream", store.config().stream_name());
    headers.insert("Nats-Batch-Sequence", "1");
    headers.insert("Nats-Batch-Id", "repeated-prefix-operation");
    headers.insert(
        "Nats-Expected-Last-Subject-Sequence",
        original.sequence.to_string(),
    );
    let staged = fixture
        .js
        .client()
        .send_request(
            original.subject,
            async_nats::Request::new()
                .payload(payload.into())
                .headers(headers)
                .timeout(Some(WAIT)),
        )
        .await?;
    assert!(staged.payload.is_empty());
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Batch-Id", "repeated-prefix-operation");
    headers.insert("Nats-Batch-Sequence", "2");
    headers.insert("Nats-Batch-Commit", "1");
    fixture
        .js
        .send_publish(
            store.config().transaction_subject("filler"),
            async_nats::jetstream::message::PublishMessage::build()
                .payload("{}".into())
                .headers(headers),
        )
        .await?
        .await?;

    // The old receipt remains valid for its original transaction only.
    assert_eq!(
        store
            .load_transaction_receipt(&OperationId::new("first")?)
            .await?
            .ok_or("original receipt")?
            .events()
            .len(),
        1
    );
    let error = store
        .load_replay(&id, Some(&checkpoint), None)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), EventStoreErrorKind::CorruptHistory);
    assert!(error.message().contains("exact provenance"), "{error}");
    Ok(())
}
