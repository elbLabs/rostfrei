#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
use async_trait::async_trait;
use rostfrei_core::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Reject(AtomicUsize);
#[async_trait]
impl CommandHandler<()> for Reject {
    type Rejection = String;
    async fn handle(&self, _: &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
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
