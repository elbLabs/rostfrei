#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
rostfrei_runtime::install_macro_support!();

struct RefuseDecision {
    store: Arc<NatsEventStore>,
    unavailable: AtomicBool,
}
#[async_trait]
impl OutcomeReader for RefuseDecision {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.store.lookup_outcome(key).await
    }
}
#[async_trait]
impl ExecutionStore for RefuseDecision {
    fn authority(&self) -> &CommandAuthority {
        self.store.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(RefuseSession {
            fault: self,
            inner: self.store.begin_command().await?,
        }))
    }
}
struct RefuseSession<'a> {
    fault: &'a RefuseDecision,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for RefuseSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for RefuseSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        if self.fault.unavailable.load(Ordering::SeqCst) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected outcome-write refusal before persistence",
            ));
        }
        self.inner.commit_outcome(proposal).await
    }
}
#[async_trait]
impl EventHistory for RefuseDecision {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.store.load(id).await
    }
}
#[async_trait]
impl EventStore for RefuseDecision {
    async fn load_completed_outcome_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<InlineCompletedOutcome>, EventStoreError> {
        self.store
            .load_completed_outcome_in_context(context, operation)
            .await
    }
    async fn commit_outcome(
        &self,
        transaction: EventTransaction,
    ) -> Result<CompletedAppendOutcome, EventStoreError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected outcome-write refusal before persistence",
            ));
        }
        self.store.commit_outcome(transaction).await
    }
    async fn append(
        &self,
        id: &StreamId,
        version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.store.append(id, version, batch).await
    }
    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.store.append_transaction(transaction).await
    }
    async fn load_transaction_receipt(
        &self,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.store.load_transaction_receipt(operation).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.store
            .load_transaction_receipt_in_context(context, operation)
            .await
    }
}

#[tokio::test]
async fn unavailable_rejection_write_is_not_completed_or_acked_and_same_id_recovers() -> TestResult
{
    let fixture = Fixture::new("92-unavailable-decision-write").await?;
    let store = fixture.split_event_store().await?;
    let refusal = Arc::new(RefuseDecision {
        store: store.clone(),
        unavailable: AtomicBool::new(true),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, refusal.clone(), calls.clone(), None)?;
    let command = local_bus(&fixture.context, processor.clone()).encode(request(
        "unavailable-rejection",
        "missing",
        "add",
    )?)?;
    let error = processor.process(&command).await.unwrap_err();
    assert_eq!(
        error.execution_failure(),
        Some(ExecutionFailure::Store(StoreFailureKind::Unavailable))
    );
    assert!(error.is_retryable());
    let handler = fixture.adapter.command_handler(processor.clone());
    assert!(matches!(
        handler.handle(command_delivery(&command)?).await,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert!(
        store
            .load_completed_outcome_in_context(
                fixture.context.name(),
                &OperationId::new(command.operation_id().as_str())?
            )
            .await?
            .is_none()
    );
    assert_eq!(
        fixture
            .messages(
                fixture
                    .messaging
                    .topology()
                    .command_response_stream()
                    .as_str()
            )
            .await?,
        0
    );
    refusal.unavailable.store(false, Ordering::SeqCst);
    assert_eq!(
        handler.handle(command_delivery(&command)?).await,
        DeliveryDisposition::Acknowledge
    );
    let before = calls.load(Ordering::SeqCst);
    assert!(rejected(processor.process(&command).await?.outcome()));
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert!(store.load(&stream("missing")?).await?.is_empty());
    Ok(())
}
