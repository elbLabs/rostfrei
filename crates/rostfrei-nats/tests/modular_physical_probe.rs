//! Runnable physical RED on the baseline. Uses only the owned authenticated fixture runner.
#![allow(clippy::panic_in_result_fn, clippy::indexing_slicing)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
rostfrei::install_macro_support!();

struct RejectBytes;
#[async_trait]
impl CommandHandler<String> for RejectBytes {
    type Rejection = String;
    async fn handle(
        &self,
        value: &String,
        _: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        Ok(CommandDecision::Rejected(value.clone()))
    }
}

#[tokio::test]
async fn permanent_journal_size_is_independent_of_rejection_body_size() -> TestResult {
    let fixture = Fixture::new("modular-physical-red").await?;
    let store = fixture.event_store().await?;
    let journal = fixture.js.get_stream(store.config().stream_name()).await?;
    let mut sizes = Vec::new();
    for (operation, length) in [("small", 32), ("large", 24_000)] {
        let metadata = CommandExecutionMetadata::new(
            OperationId::new(operation)?,
            ContentFingerprint::digest(operation),
        )
        .with_bounded_context(fixture.context.name().clone());
        let body = "s".repeat(length);
        assert_eq!(
            CommandExecutor::new(store.clone())
                .execute_with_rejection_codec(
                    &RejectBytes,
                    metadata,
                    &body,
                    &JsonRejectionCodec::new("test.secret", 1)
                )
                .await?,
            CommandOutcome::Rejected(body)
        );
        let mut journal_info = fixture.js.get_stream(store.config().stream_name()).await?;
        let sequence = journal_info.info().await?.state.last_sequence;
        let record = journal.get_raw_message(sequence).await?;
        sizes.push(record.payload.len());
        assert!(record.subject.contains("transaction"));
    }
    eprintln!(
        "permanent journal payload sizes: small={} large={}",
        sizes[0], sizes[1]
    );
    assert!(
        sizes[1].abs_diff(sizes[0]) < 512,
        "LIFE-2/physical §13.1: permanent journal grew with retainable rejection bytes: {sizes:?}"
    );
    Ok(())
}
