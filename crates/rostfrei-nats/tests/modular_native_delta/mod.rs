//! FR-1..4 delta. Opaque product records are captured, never reconstructed from guessed fields.
use super::*;
use async_nats::jetstream::{
    context::PublishErrorKind,
    stream::{
        Config, DiscardPolicy, PersistenceMode, RawMessageErrorKind, RetentionPolicy, StorageType,
        Stream,
    },
};
use modular_state_fault_support::StateProxy;
use rostfrei::{CommandBusErrorKind, EncodedCommand};
use serde::{Deserialize, Serialize};

pub(super) fn assert_state_config(config: &Config) {
    assert_eq!(config.storage, StorageType::File);
    assert_eq!(config.retention, RetentionPolicy::Limits);
    assert_eq!(config.discard, DiscardPolicy::New);
    assert!(!config.discard_new_per_subject);
    assert_eq!(config.max_messages_per_subject, 1);
    assert_eq!(config.max_messages, -1);
    assert_eq!(config.max_age, Duration::ZERO);
    assert!(config.max_bytes > 0 && config.max_message_size > 0);
    assert!(config.deny_delete && config.deny_purge);
    assert!(!config.allow_rollup && !config.allow_direct && !config.no_ack);
    assert!(!config.allow_message_ttl && !config.allow_message_schedules);
    assert!(config.subject_delete_marker_ttl.is_none());
    assert!(config.sources.is_none() && config.mirror.is_none());
    assert!(config.subject_transform.is_none());
    assert_ne!(config.persist_mode, Some(PersistenceMode::Async));
}

/// Only public broker evidence; serialization is for the owned restart child.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct StateRecord {
    subject: String,
    sequence: u64,
    payload: Vec<u8>,
}
impl From<async_nats::jetstream::message::StreamMessage> for StateRecord {
    fn from(message: async_nats::jetstream::message::StreamMessage) -> Self {
        Self {
            subject: message.subject.to_string(),
            sequence: message.sequence,
            payload: message.payload.to_vec(),
        }
    }
}
pub(super) async fn capture_latest(states: &mut Stream) -> TestResult<StateRecord> {
    let sequence = states.info().await?.state.last_sequence;
    Ok(states.get_raw_message(sequence).await?.into())
}
pub(super) async fn assert_replaced(
    states: &Stream,
    active: &StateRecord,
) -> TestResult<StateRecord> {
    let current: StateRecord = states
        .get_last_raw_message_by_subject(&active.subject)
        .await?
        .into();
    assert!(current.sequence > active.sequence);
    assert_ne!(current.payload, active.payload);
    assert_eq!(
        states
            .get_raw_message(active.sequence)
            .await
            .unwrap_err()
            .kind(),
        RawMessageErrorKind::NoMessageFound,
        "retained old payload sequence must be absent, not merely hidden by lookup"
    );
    Ok(current)
}
pub(super) async fn assert_prepare_refused(
    js: &async_nats::jetstream::Context,
    stream: &str,
    active: &StateRecord,
) -> TestResult {
    // Replay EXACT captured Active bytes as a delayed conforming normal prepare.
    // No dedup id, rollup or KV helper; this must fail at subject CAS, not dedup.
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Expected-Stream", stream);
    headers.insert("Nats-Expected-Last-Subject-Sequence", "0");
    let error = js
        .publish_with_headers(
            active.subject.clone(),
            headers,
            active.payload.clone().into(),
        )
        .await?
        .await
        .unwrap_err();
    assert_eq!(error.kind(), PublishErrorKind::WrongLastSequence);
    Ok(())
}
fn service(
    fixture: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<dyn CommandProcessingService>> {
    let mut processor = CommandProcessor::builder(fixture.context.clone(), store)
        .authorizer(Arc::new(Policy {
            deny_read: AtomicBool::new(false),
            principals: std::sync::Mutex::new(Vec::new()),
        }))
        .build()?;
    processor.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(Arc::new(processor))
}
fn admitted_bus(
    fixture: &Fixture,
    service: Arc<dyn CommandProcessingService>,
) -> TestResult<CommandBus> {
    Ok(CommandBus::new(
        fixture.context.clone(),
        Arc::new(
            NatsMessagingAdapter::new(
                fixture.publisher(),
                NatsCommandResponseReader::new(
                    fixture.js.clone(),
                    fixture.messaging.topology().clone(),
                ),
            )
            .with_outcome_service(service, caller()?)
            .with_response_timeout(Duration::from_secs(2)),
        ),
    ))
}
fn command(fixture: &Fixture, id: &str) -> TestResult<EncodedCommand> {
    Ok(
        CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
            .encode(request(id, "target", "noop")?)?,
    )
}
async fn cache(
    fixture: &Fixture,
    command: &EncodedCommand,
    response: &CommandResponse,
) -> TestResult {
    fixture
        .publisher()
        .publish_command_response(
            OutboundMessage::json(
                command.response_address()?,
                response.message_id().clone(),
                response,
            )?
            .with_correlation_id(command.correlation_id().clone()),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn stale_maintenance_revision_is_write_free_and_delayed_active_cannot_resurrect() -> TestResult
{
    let fixture = Fixture::new("modular-stale-and-delayed").await?;
    let store = split_store(&fixture).await?;
    let executor = CommandExecutor::new(store.clone());
    for id in ["winner", "unrelated"] {
        executor
            .execute_with_rejection_codec(
                &LargeReject,
                CommandExecutionMetadata::new(
                    OperationId::new(id)?,
                    ContentFingerprint::digest(id),
                )
                .with_bounded_context(fixture.context.name().clone()),
                &(),
                &JsonRejectionCodec::new("large", 1),
            )
            .await?;
    }
    let winner_key = key(&store, &fixture.context, &OperationId::new("winner")?);
    let unrelated_key = key(&store, &fixture.context, &OperationId::new("unrelated")?);
    let OutcomeLookup::Completed(winner) = store.lookup_outcome(&winner_key).await? else {
        return Err("winner missing".into());
    };
    let OutcomeLookup::Completed(unrelated) = store.lookup_outcome(&unrelated_key).await? else {
        return Err("control winner missing".into());
    };
    assert_ne!(winner.marker().revision(), unrelated.marker().revision());
    let state_name = store.config().outcome_state_stream_name();
    let mut states = fixture.js.get_stream(state_name).await?;
    assert_state_config(&states.cached_info().config);
    let before = states.info().await?.state.clone();
    assert_eq!(before.messages, 2);
    // This isolated fixture prepared winner first, unrelated second, with no other writes.
    let active: StateRecord = states.get_raw_message(before.first_sequence).await?.into();
    let other = capture_latest(&mut states).await?;
    assert!(
        store
            .expire_details(&winner_key, unrelated.marker().revision())
            .await
            .is_err()
    );
    assert_eq!(
        states.info().await?.state.last_sequence,
        before.last_sequence
    );
    assert_eq!(states.info().await?.state.messages, before.messages);
    assert_eq!(
        StateRecord::from(states.get_raw_message(active.sequence).await?),
        active
    );
    assert_eq!(
        StateRecord::from(states.get_raw_message(other.sequence).await?),
        other
    );

    let expiry = store
        .expire_details(&winner_key, winner.marker().revision())
        .await?;
    assert_eq!(expiry.reclamation(), ReclamationStatus::Reclaimed);
    let expired = assert_replaced(&states, &active).await?;
    assert!(
        expired.payload.len() < active.payload.len() / 4,
        "only bounded expiry evidence remains"
    );
    assert!(states.info().await?.state.bytes < before.bytes);
    assert_prepare_refused(&fixture.js, state_name, &active).await?;
    assert_eq!(assert_replaced(&states, &active).await?, expired);
    assert_eq!(
        StateRecord::from(states.get_raw_message(other.sequence).await?),
        other
    );
    assert_eq!(states.info().await?.state.messages, 2);
    assert!(
        matches!(store.lookup_outcome(&winner_key).await?, OutcomeLookup::Completed(outcome)
        if matches!(outcome.details(), OutcomeDetails::Expired))
    );
    assert_eq!(
        message_count(&fixture, store.config().stream_name()).await?,
        2
    );
    Ok(())
}

struct HotMiss;
#[async_trait]
impl OutcomeDetailReader for HotMiss {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        Ok(DetailRead::AbsentHere)
    }
}
/// An independent exact-copy reader: no store, receipt, service, or lifecycle accessor.
struct ColdCopy {
    reference: OutcomeDetailReference,
    bytes: Vec<u8>,
    calls: AtomicUsize,
}
#[async_trait]
impl OutcomeDetailReader for ColdCopy {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(if reference == &self.reference {
            DetailRead::Found(self.bytes.clone())
        } else {
            DetailRead::AbsentHere
        })
    }
}

#[tokio::test]
async fn lost_state_authority_blocks_warm_cache_and_independent_cold_copy_without_execution()
-> TestResult {
    for fault in ["unavailable", "missing", "recreated"] {
        let fixture = Fixture::new(&format!("modular-state-{fault}")).await?;
        let store = split_store(&fixture).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let original = service(&fixture, store.clone(), calls.clone())?;
        let command = command(&fixture, "state-authority")?;
        let completed = original.process(&caller()?, &command).await.into_result()?;
        let OutcomeDetails::Available(response) = completed.outcome().details() else {
            return Err("fresh response unavailable".into());
        };
        cache(&fixture, &command, response).await?;
        let reference = completed.outcome().marker().detail_reference().clone();
        let DetailRead::Found(bytes) = store.detail_reader().get(&reference).await? else {
            return Err("copy source missing".into());
        };
        let cold = Arc::new(ColdCopy {
            reference,
            bytes,
            calls: AtomicUsize::new(0),
        });
        let state_name = store.config().outcome_state_stream_name();
        let mut states = fixture.js.get_stream(state_name).await?;
        let config = states.cached_info().config.clone();
        let active = capture_latest(&mut states).await?;
        let proxy = StateProxy::start(state_name, &active.subject, active.sequence).await?;
        let guarded_store = Arc::new(
            NatsEventStore::connect(
                proxy.js.clone(),
                store.config().clone().with_explicit_detail_expiry(false)?,
            )
            .await?
            .with_detail_readers(Arc::new(HotMiss), Some(cold.clone())),
        );
        let guarded = service(&fixture, guarded_store, calls.clone())?;
        let bus = admitted_bus(&fixture, guarded.clone())?;
        assert_eq!(
            bus.dispatch_encoded(command.clone()).await?.response(),
            response
        );
        assert!(
            cold.calls.load(Ordering::SeqCst) > 0,
            "control must really consult independent cold storage"
        );
        let cold_before = cold.calls.load(Ordering::SeqCst);
        let journal_before = message_count(&fixture, store.config().stream_name()).await?;
        if fault == "unavailable" {
            proxy.faults.block_state_reads.store(true, Ordering::SeqCst);
        } else {
            // Destructive loss of this fixture's whole authority, NOT hot-only eviction.
            fixture.js.delete_stream(state_name).await?;
            if fault == "recreated" {
                fixture.js.create_stream(config).await?;
            }
        }
        // Both calls start AFTER a previously successful admission and authority loss.
        let lookup = tokio::time::timeout(WAIT, guarded.lookup(&caller()?, &command))
            .await?
            .into_result();
        match lookup {
            Err(_) => {}
            Ok(OutcomeLookup::Completed(outcome)) => assert!(matches!(
                outcome.details(),
                OutcomeDetails::Unavailable { .. }
            )),
            Ok(OutcomeLookup::Missing) => {
                return Err("lost state authority became an executable miss".into());
            }
        }
        assert!(
            tokio::time::timeout(WAIT, bus.dispatch_encoded(command.clone()))
                .await?
                .is_err()
        );
        assert_eq!(
            cold.calls.load(Ordering::SeqCst),
            cold_before,
            "cold bytes must not bypass lost lifecycle authority"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            message_count(&fixture, store.config().stream_name()).await?,
            journal_before
        );
        if fault == "unavailable" {
            assert!(proxy.faults.dropped_reads.load(Ordering::SeqCst) >= 2);
            proxy
                .faults
                .block_state_reads
                .store(false, Ordering::SeqCst);
            assert_eq!(
                bus.dispatch_encoded(command.clone()).await?.response(),
                response
            );
        }
        let raw = NatsCommandResponseReader::new(
            fixture.js.clone(),
            fixture.messaging.topology().clone(),
        )
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(1),
        )
        .await?;
        assert_eq!(&raw, response, "transport copy is still present throughout");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn active_winner_does_not_admit_wrong_identity_or_wrong_decision_raw_response() -> TestResult
{
    for wrong_identity in [false, true] {
        let fixture = Fixture::new(if wrong_identity {
            "modular-wrong-identity"
        } else {
            "modular-wrong-decision"
        })
        .await?;
        let store = split_store(&fixture).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(&fixture, store.clone(), calls.clone())?;
        let command = command(&fixture, "authorized-winner")?;
        let completed = service.process(&caller()?, &command).await.into_result()?;
        let OutcomeDetails::Available(winner) = completed.outcome().details() else {
            return Err("fresh winner unavailable".into());
        };
        assert!(matches!(winner.outcome(), CommandResponseOutcome::Accepted));
        let candidate = if wrong_identity {
            let other = self::command(&fixture, "different-operation")?;
            CommandResponse::accepted(
                rostfrei::command_response_message_id(other.message_id())?,
                other.message_id().clone(),
                other.address().clone(),
                other.operation_id().clone(),
                other.correlation_id().clone(),
            )?
        } else {
            CommandResponse::rejected(
                winner.message_id().clone(),
                winner.command_message_id().clone(),
                winner.command_address().clone(),
                winner.operation_id().clone(),
                winner.correlation_id().clone(),
                CommandRejection::new(
                    CommandRejectionClassification::Conflict,
                    ApplicationErrorCode::new("delta.false-rejection")?,
                    "not the journal winner",
                    None,
                )?,
            )?
        };
        assert_ne!(&candidate, winner);
        // Deliberately publish raw on the requested response subject, bypassing publisher
        // validation, so admission is tested against real contradictory retained evidence.
        let payload = serde_json::to_vec(&candidate)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Content-Type", "application/json");
        headers.insert("Nats-Msg-Id", candidate.message_id().as_str());
        headers.insert(CORRELATION_ID_HEADER, command.correlation_id().as_str());
        fixture
            .js
            .publish_with_headers(
                command.response_address()?.to_string(),
                headers,
                payload.clone().into(),
            )
            .await?
            .await?;
        if !wrong_identity {
            let raw = NatsCommandResponseReader::new(
                fixture.js.clone(),
                fixture.messaging.topology().clone(),
            )
            .read_command_response(
                &command.response_address()?,
                command.operation_id(),
                command.message_id(),
                Duration::from_secs(1),
            )
            .await?;
            assert_eq!(
                raw, candidate,
                "raw #85 identity checks alone cannot detect a different decision"
            );
        }
        let bus = admitted_bus(&fixture, service.clone())?;
        match bus.dispatch_encoded(command.clone()).await {
            Ok(receipt) => assert_eq!(
                receipt.response(),
                winner,
                "only verified authoritative bytes may be returned"
            ),
            Err(error) => assert_eq!(error.kind(), CommandBusErrorKind::InvalidResponse),
        }
        let OutcomeLookup::Completed(after) =
            service.lookup(&caller()?, &command).await.into_result()?
        else {
            return Err("admission changed permanent winner".into());
        };
        assert!(
            matches!(after.details(), OutcomeDetails::Available(response) if response == winner)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let responses = fixture
            .js
            .get_stream(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str(),
            )
            .await?;
        assert_eq!(
            responses
                .get_last_raw_message_by_subject(command.response_address()?.as_str())
                .await?
                .payload
                .as_ref(),
            payload
        );
    }
    Ok(())
}

#[tokio::test]
async fn lost_expiry_ack_and_failed_old_sequence_verification_remain_withdrawn_and_retryable()
-> TestResult {
    for lose_ack in [false, true] {
        let fixture = Fixture::new(if lose_ack {
            "modular-expiry-ack"
        } else {
            "modular-expiry-verification"
        })
        .await?;
        let store = split_store(&fixture).await?;
        CommandExecutor::new(store.clone())
            .execute_with_rejection_codec(
                &LargeReject,
                metadata(&fixture.context)?,
                &(),
                &JsonRejectionCodec::new("large", 1),
            )
            .await?;
        let outcome_key = key(
            &store,
            &fixture.context,
            &OperationId::new("prepared-only")?,
        );
        let OutcomeLookup::Completed(outcome) = store.lookup_outcome(&outcome_key).await? else {
            return Err("seed decision missing".into());
        };
        let state_name = store.config().outcome_state_stream_name();
        let mut states = fixture.js.get_stream(state_name).await?;
        let active = capture_latest(&mut states).await?;
        let proxy = StateProxy::start(state_name, &active.subject, active.sequence).await?;
        let maintenance = NatsEventStore::connect(
            proxy.js.clone(),
            store
                .config()
                .clone()
                .with_puback_timeout(Duration::from_millis(500))?,
        )
        .await?;
        proxy
            .faults
            .lose_expiry_acks
            .store(lose_ack, Ordering::SeqCst);
        proxy
            .faults
            .block_old_sequence
            .store(!lose_ack, Ordering::SeqCst);
        let result = tokio::time::timeout(
            WAIT,
            maintenance.expire_details(&outcome_key, outcome.marker().revision()),
        )
        .await?;
        if !lose_ack {
            assert!(
                proxy.faults.dropped_reads.load(Ordering::SeqCst) > 0,
                "must attempt old-sequence verification before claiming reclamation"
            );
            if let Ok(expiry) = result {
                assert_eq!(expiry.reclamation(), ReclamationStatus::Pending);
            } // A typed verification failure is also permitted; it cannot restore Active.
        } else {
            assert!(
                proxy.faults.dropped_acks.load(Ordering::SeqCst) > 0,
                "real PubAck must have been suppressed"
            );
        }
        let expired = assert_replaced(&states, &active).await?;
        let fresh = NatsEventStore::connect(
            fixture.js.clone(),
            store.config().clone().with_explicit_detail_expiry(false)?,
        )
        .await?;
        assert!(
            matches!(fresh.lookup_outcome(&outcome_key).await?, OutcomeLookup::Completed(current)
            if matches!(current.details(), OutcomeDetails::Expired))
        );
        proxy.faults.lose_expiry_acks.store(false, Ordering::SeqCst);
        proxy
            .faults
            .block_old_sequence
            .store(false, Ordering::SeqCst);
        let retry = maintenance
            .expire_details(&outcome_key, outcome.marker().revision())
            .await?;
        assert_eq!(retry.reclamation(), ReclamationStatus::Reclaimed);
        assert_eq!(assert_replaced(&states, &active).await?, expired);
        assert_prepare_refused(&fixture.js, state_name, &active).await?;
        assert_eq!(states.info().await?.state.messages, 1);
        assert_eq!(
            message_count(&fixture, store.config().stream_name()).await?,
            1
        );
    }
    Ok(())
}
