//! Independent integrated review: sender admission and hostile version dispatch.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::too_many_lines
)]
#[path = "../src/event_store_config.rs"]
mod event_store_config;
#[path = "../src/hex.rs"]
mod hex;
mod release_contract_support;
#[path = "../src/stream_policy.rs"]
mod stream_policy;
#[path = "../src/subject_tokens.rs"]
mod subject_tokens;
mod event_store {
    mod independent {
        use super::*;

        #[test]
        fn review_actor_receipt_dispatch_rejects_rechecksummed_downgrades_and_duplicate_claims() {
            let context = rostfrei_messaging_core::ApplicationName::new("review-wire")
                .unwrap()
                .bounded_context("orders")
                .unwrap();
            let config = NatsEventStoreConfig::for_bounded_context(&context).unwrap();
            let mut headers = HeaderMap::new();
            for (key, value) in [
                ("Content-Type", "application/json"),
                ("Nats-Batch-Id", "review-wire"),
                ("Nats-Batch-Sequence", "1"),
                ("Nats-Batch-Commit", NATS_BATCH_COMMIT_FINAL),
                ("Nats-Expected-Last-Subject-Sequence", "0"),
                ("Nats-Expected-Stream", config.stream_name()),
            ] {
                headers.insert(key, value);
            }
            let template = TransactionReceiptContentWire {
                event_store_stream: config.stream_name().to_owned(),
                application: context.application().as_str().to_owned(),
                bounded_context: context.name().as_str().to_owned(),
                operation_id: "review".to_owned(),
                operation_fingerprint: ContentFingerprint::digest("explicit").to_hex(),
                correlation_id: None,
                causation_id: None,
                security_scope: None,
                actor: Some(rostfrei_core::ActorRef::new("people", "alice").unwrap()),
                completion: None,
                manifest: None,
                participants: vec![],
            };
            let valid = encode_transaction_receipt(&template).unwrap();
            decode_transaction_receipt(&config, 1, &headers, &valid).unwrap();
            // Recompute checksums so a stale checksum cannot be the only rejecting gate.
            for schema_version in [0, 1, 2, 3, 4, 5, 6, u16::MAX] {
                for present in [false, true] {
                    let mut receipt = template.clone();
                    if !present {
                        receipt.actor = None;
                    }
                    let wire = StoredTransactionReceiptWire {
                        checksum: transaction_receipt_checksum(schema_version, &receipt).unwrap(),
                        schema_version,
                        receipt,
                    };
                    let result = decode_transaction_receipt(
                        &config,
                        1,
                        &headers,
                        &serde_json::to_vec(&wire).unwrap(),
                    );
                    if (schema_version == 2 && !present) || (schema_version == 5 && present) {
                        result.unwrap();
                    } else {
                        assert_eq!(
                            result.err().unwrap().kind(),
                            EventStoreErrorKind::CorruptHistory,
                            "schema={schema_version}, actor={present}"
                        );
                    }
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
    }
    include!("../src/event_store.rs");
}

use async_trait::async_trait;
use release_contract_support::*;
use rostfrei_runtime::{
    AuthorizationError, CommandAccess, CommandAuthorizer, CommandProcessingService, PrincipalId,
    TrustedCommandContext, ValidatedCommand,
};
rostfrei_runtime::install_macro_support!();

type AccessRow = (String, Option<ActorRef>, CommandAccess);
#[derive(Default)]
struct Policy(std::sync::Mutex<Vec<AccessRow>>);
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.0.lock().unwrap().push((
            caller.principal().as_str().to_owned(),
            command.original().actor().cloned(),
            access,
        ));
        Ok(())
    }
}
fn caller(name: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new(name)?,
        SecurityScope::new("same-realm")?,
    ))
}
fn actor() -> TestResult<ActorRef> {
    Ok(ActorRef::new("people", "alice")?)
}

#[tokio::test]
async fn review_actor_sender_reauthorizes_original_claim_and_never_admits_changed_or_expired_cache()
-> TestResult {
    let f = Fixture::new("review-actor-sender").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?
        .with_fresh_split_outcome_epoch("review-actor")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let mut p = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(policy.clone())
        .build()?;
    p.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let service = Arc::new(p);
    let raw_bus = CommandBus::new(f.context.clone(), f.adapter.clone());
    let command = raw_bus.encode(request("same-op", "target", "create")?.with_actor(actor()?))?;
    let worker = f.adapter.command_handler_with_resolver(
        service.clone(),
        Arc::new(ServicePrincipalContext(caller("worker")?)),
    );
    assert_eq!(
        worker.handle(command_delivery(&command)?).await,
        DeliveryDisposition::Acknowledge
    );
    let response = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone())
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            WAIT,
        )
        .await?;
    let sender = NatsMessagingAdapter::new(
        f.publisher(),
        NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone()),
    )
    .with_outcome_service(service.clone(), caller("different-sender")?)
    .with_response_timeout(Duration::from_secs(2));
    let bus = CommandBus::new(f.context.clone(), Arc::new(sender));
    policy.0.lock().unwrap().clear();
    assert_eq!(
        bus.dispatch_encoded(command.clone()).await?.response(),
        &response
    );
    let before = (
        f.messages(store.config().stream_name()).await?,
        f.messages(store.config().outcome_state_stream_name())
            .await?,
    );
    let changed = raw_bus.encode(
        request("same-op", "target", "create")?.with_actor(ActorRef::new("People", "alice")?),
    )?;
    let omitted = raw_bus.encode(request("same-op", "target", "create")?)?;
    for invalid in [changed, omitted] {
        assert_eq!(invalid.operation_id(), command.operation_id());
        assert_eq!(
            bus.dispatch_encoded(invalid).await.unwrap_err().kind(),
            rostfrei_runtime::CommandBusErrorKind::InvalidMessage
        );
    }
    let winner = service
        .lookup(&caller("maintenance-reader")?, &command)
        .await
        .into_result()?;
    let OutcomeLookup::Completed(winner) = winner else {
        return Err("winner missing".into());
    };
    store
        .expire_details(
            winner.marker().detail_reference().key(),
            winner.marker().revision(),
        )
        .await?;
    assert_eq!(
        bus.dispatch_encoded(command.clone())
            .await
            .unwrap_err()
            .kind(),
        rostfrei_runtime::CommandBusErrorKind::CompletedDetailsExpired
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        (
            f.messages(store.config().stream_name()).await?,
            f.messages(store.config().outcome_state_stream_name())
                .await?
        ),
        before
    );
    let rows = policy.0.lock().unwrap();
    assert!(
        rows.iter()
            .all(|(_, _, phase)| *phase == CommandAccess::ReadOutcome)
    );
    assert!(
        rows.iter()
            .filter(|(name, claim, _)| name == "different-sender"
                && claim.as_ref() == Some(&actor().unwrap()))
            .count()
            >= 2
    );
    assert!(rows.iter().any(|(_, claim, _)| claim.is_none()));
    drop(rows);
    Ok(())
}

#[tokio::test]
async fn review_actor_guard_only_same_fingerprint_cannot_prepare_another_claim_after_expiry()
-> TestResult {
    let f = Fixture::new("review-actor-guards").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?
        .with_fresh_split_outcome_epoch("review-guard")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    let store = NatsEventStore::connect(f.js.clone(), config).await?;
    let operation = OperationId::new("guard-only")?;
    let fingerprint = ContentFingerprint::digest("caller supplied constant");
    let tx = EventTransaction::new(
        operation.clone(),
        fingerprint,
        vec![TransactionParticipant::new(
            stream("guard")?,
            ExpectedVersion::NoStream,
            None,
        )],
    )
    .with_bounded_context(f.context.name().clone())
    .with_actor(actor()?)
    .with_completion(CommandCompletion::new(
        CommandExecutionMetadata::new(operation, fingerprint)
            .with_default_outcome_evidence::<()>()
            .outcome_evidence()
            .cloned(),
        None,
    ));
    let winner = store
        .begin_command()
        .await?
        .commit_outcome(tx.clone())
        .await?;
    let marker = winner.outcome().marker();
    assert_eq!(marker.integrity_receipt().streams().len(), 1);
    assert!(
        marker
            .integrity_receipt()
            .streams()
            .first()
            .unwrap()
            .events()
            .is_empty()
    );
    assert_eq!(marker.actor(), Some(&actor()?));
    for expired in [false, true] {
        if expired {
            store
                .expire_details(marker.detail_reference().key(), marker.revision())
                .await?;
        }
        let before = (
            f.messages(store.config().stream_name()).await?,
            f.messages(store.config().outcome_state_stream_name())
                .await?,
        );
        let changed = tx.clone().with_actor(ActorRef::new("people", "ALICE")?);
        assert_eq!(
            store
                .begin_command()
                .await?
                .commit_outcome(changed)
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
        );
        assert_eq!(
            (
                f.messages(store.config().stream_name()).await?,
                f.messages(store.config().outcome_state_stream_name())
                    .await?
            ),
            before
        );
        let OutcomeLookup::Completed(recovered) = store
            .lookup_outcome(marker.detail_reference().key())
            .await?
        else {
            return Err("lost winner".into());
        };
        assert_eq!(recovered.marker(), marker);
        assert_eq!(
            matches!(recovered.details(), OutcomeDetails::Expired),
            expired
        );
    }
    assert!(store.audit_history(&stream("guard")?).await?.is_empty());
    Ok(())
}
