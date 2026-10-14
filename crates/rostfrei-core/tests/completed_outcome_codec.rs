#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
use async_trait::async_trait;
use rostfrei_core::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Reject(AtomicUsize);
#[async_trait]
impl CommandHandler<()> for Reject {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Rejected("prerequisite missing".to_owned()))
    }
}

#[async_trait]
impl CommandHandler<bool> for Reject {
    type Rejection = String;
    async fn handle(
        &self,
        _: &bool,
        _: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Rejected("prerequisite missing".to_owned()))
    }
}
fn metadata() -> CommandExecutionMetadata {
    CommandExecutionMetadata::new(
        OperationId::new("generic-operation").unwrap(),
        ContentFingerprint::digest("arbitrary-generic-command"),
    )
    .with_bounded_context(rostfrei_messaging_core::BoundedContextName::new("generic").unwrap())
}

#[tokio::test]
async fn explicit_generic_codec_survives_executor_recreation_and_blocks_unknown_codec() {
    let store = InMemoryEventStore::new();
    let handler = Reject(AtomicUsize::new(0));
    let codec = JsonRejectionCodec::new("app.generic-rejection", 1);
    let first = CommandExecutor::new(store.clone())
        .execute_with_rejection_codec(&handler, metadata(), &(), &codec)
        .await
        .unwrap();
    let second = CommandExecutor::new(store.clone())
        .execute_with_rejection_codec(&handler, metadata(), &(), &codec)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
    let record = store
        .load_completed_outcome_in_context(
            metadata().bounded_context().unwrap(),
            metadata().operation_id(),
        )
        .await
        .unwrap()
        .unwrap();
    let evidence = record
        .record()
        .completion()
        .unwrap()
        .evidence
        .as_ref()
        .unwrap();
    assert_eq!(evidence.command_type, "rust:()");
    assert_eq!(evidence.command_schema, 1);
    assert_eq!(evidence.fingerprint_version, 1);
    assert!(!evidence.events_caused_by_command);
    let error = CommandExecutor::new(store)
        .execute_with_rejection_codec(
            &handler,
            metadata(),
            &(),
            &JsonRejectionCodec::new("app.generic-rejection", 2),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Codec(error) if error.kind() == EventCodecErrorKind::UnsupportedSchemaVersion)
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn generic_default_type_identity_cannot_be_reused_for_another_command_type() {
    let store = InMemoryEventStore::new();
    let handler = Reject(AtomicUsize::new(0));
    let codec = JsonRejectionCodec::new("app.generic-rejection", 1);
    let executor = CommandExecutor::new(store);
    executor
        .execute_with_rejection_codec(&handler, metadata(), &(), &codec)
        .await
        .unwrap();
    let error = executor
        .execute_with_rejection_codec(&handler, metadata(), &true, &codec)
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Store(error) if error.kind() == EventStoreErrorKind::IdentityConflict)
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_codec_fails_closed_and_does_not_cache_rejection() {
    let store = InMemoryEventStore::new();
    let error = CommandExecutor::new(store.clone())
        .execute(&Reject(AtomicUsize::new(0)), metadata(), &())
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Codec(error) if error.kind() == EventCodecErrorKind::EncodingFailed)
    );
    assert!(
        store
            .load_completed_outcome_in_context(
                metadata().bounded_context().unwrap(),
                metadata().operation_id()
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn codec_refuses_oversize_unknown_identity_version_and_malformed_bytes() {
    let codec = JsonRejectionCodec::new("app.generic-rejection", 1);
    assert!(
        <JsonRejectionCodec as RejectionCodec<String>>::encode(
            &codec,
            &"x".repeat(MAX_REJECTION_BYTES + 1)
        )
        .is_err()
    );
    let encoded =
        <JsonRejectionCodec as RejectionCodec<String>>::encode(&codec, &"valid".to_owned())
            .unwrap();
    for changed in [
        EncodedRejection {
            codec_id: "other".to_owned(),
            ..encoded.clone()
        },
        EncodedRejection {
            codec_version: 2,
            ..encoded.clone()
        },
        EncodedRejection {
            bytes: b"malformed".to_vec(),
            ..encoded
        },
    ] {
        assert!(<JsonRejectionCodec as RejectionCodec<String>>::decode(&codec, &changed).is_err());
    }
}

#[tokio::test]
async fn rejection_capacity_failure_is_technical_and_same_identity_recovers() {
    let store = InMemoryEventStore::new().with_outcome_capacity(0);
    let handler = Reject(AtomicUsize::new(0));
    let codec = JsonRejectionCodec::new("app.generic-rejection", 1);
    let error = CommandExecutor::new(store.clone())
        .execute_with_rejection_codec(&handler, metadata(), &(), &codec)
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::RejectedOutcomePersistence(ref error) if error.kind() == EventStoreErrorKind::CapacityExhausted)
    );
    assert!(
        store
            .load_completed_outcome_in_context(
                metadata().bounded_context().unwrap(),
                metadata().operation_id()
            )
            .await
            .unwrap()
            .is_none()
    );
    let recovered = store.with_outcome_capacity(1);
    assert!(matches!(
        CommandExecutor::new(recovered)
            .execute_with_rejection_codec(&handler, metadata(), &(), &codec)
            .await
            .unwrap(),
        CommandOutcome::Rejected(_)
    ));
    assert_eq!(handler.0.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn stored_malformed_rejection_blocks_replay_before_handler() {
    let store = InMemoryEventStore::new();
    let encoded = EncodedRejection {
        codec_id: "app.generic-rejection".to_owned(),
        codec_version: 1,
        bytes: b"not-json".to_vec(),
    };
    let transaction = EventTransaction::new(
        metadata().operation_id().clone(),
        metadata().operation_fingerprint(),
        Vec::new(),
    )
    .with_bounded_context(metadata().bounded_context().unwrap().clone())
    .with_correlation_id(
        rostfrei_messaging_core::CorrelationId::new(metadata().operation_id().as_str()).unwrap(),
    )
    .with_completion(CommandCompletion::new(
        metadata()
            .with_default_outcome_evidence::<()>()
            .outcome_evidence()
            .cloned(),
        Some(encoded),
    ));
    store.commit_outcome(transaction).await.unwrap();
    let handler = Reject(AtomicUsize::new(0));
    let error = CommandExecutor::new(store)
        .execute_with_rejection_codec(
            &handler,
            metadata(),
            &(),
            &JsonRejectionCodec::new("app.generic-rejection", 1),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Codec(error) if error.kind() == EventCodecErrorKind::MalformedPayload)
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rejected_identity_is_context_scoped_like_acceptance() {
    let store = InMemoryEventStore::new();
    let first = metadata();
    let codec = JsonRejectionCodec::new("app.generic-rejection", 1);
    CommandExecutor::new(store.clone())
        .execute_with_rejection_codec(&Reject(AtomicUsize::new(0)), first.clone(), &(), &codec)
        .await
        .unwrap();
    let other_context = rostfrei_messaging_core::BoundedContextName::new("other-context").unwrap();
    let other = EventTransaction::new(
        first.operation_id().clone(),
        first.operation_fingerprint(),
        Vec::new(),
    )
    .with_bounded_context(other_context.clone())
    .with_completion(CommandCompletion::new(
        metadata()
            .with_default_outcome_evidence::<()>()
            .outcome_evidence()
            .cloned(),
        None,
    ));
    store.commit_outcome(other).await.unwrap();
    assert!(
        store
            .load_completed_outcome_in_context(
                first.bounded_context().unwrap(),
                first.operation_id()
            )
            .await
            .unwrap()
            .unwrap()
            .rejection()
            .is_some()
    );
    assert!(
        store
            .load_completed_outcome_in_context(&other_context, first.operation_id())
            .await
            .unwrap()
            .unwrap()
            .acceptance()
            .is_some()
    );
}
