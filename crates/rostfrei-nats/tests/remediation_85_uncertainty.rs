//! Technical diagnostics do not prove non-commit; delivery budgets are not infinite retries.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei_runtime::{CommandBusErrorKind, ExecutionFailure, StoreFailureKind};

struct PostCommitFault {
    inner: Arc<NatsEventStore>,
    fail_once: AtomicBool,
}
#[async_trait]
impl OutcomeReader for PostCommitFault {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.inner.lookup_outcome(key).await
    }
}
#[async_trait]
impl ExecutionStore for PostCommitFault {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(PostCommitSession {
            fault: self,
            inner: self.inner.begin_command().await?,
        }))
    }
}
struct PostCommitSession<'a> {
    fault: &'a PostCommitFault,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for PostCommitSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for PostCommitSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        let outcome = self.inner.commit_outcome(proposal).await?;
        if self.fault.fail_once.swap(false, Ordering::SeqCst) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected postcommit result-read failure",
            ));
        }
        Ok(outcome)
    }
}
#[async_trait]
impl EventHistory for PostCommitFault {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl EventStore for PostCommitFault {
    async fn load_completed_outcome_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<rostfrei_core::InlineCompletedOutcome>, EventStoreError> {
        self.inner
            .load_completed_outcome_in_context(context, operation)
            .await
    }
    async fn commit_outcome(
        &self,
        transaction: EventTransaction,
    ) -> Result<rostfrei_core::CompletedAppendOutcome, EventStoreError> {
        let outcome = self.inner.commit_outcome(transaction).await?;
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected postcommit result-read failure",
            ));
        }
        Ok(outcome)
    }
    async fn append(
        &self,
        id: &StreamId,
        version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.inner.append(id, version, batch).await
    }
    async fn load_transaction_receipt(
        &self,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner.load_transaction_receipt(operation).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner
            .load_transaction_receipt_in_context(context, operation)
            .await
    }
    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        let outcome = self.inner.append_transaction(transaction).await?;
        if self.fail_once.swap(false, Ordering::SeqCst) {
            // Fault injection AFTER an actual atomic NATS commit. This models failure to return
            // a materialized result; it is not an injection into NatsEventStore's internal read.
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "injected postcommit result-read failure",
            ));
        }
        Ok(outcome)
    }
}

#[tokio::test]
async fn postcommit_technical_diagnostic_reconciles_same_identity_without_second_effect()
-> TestResult {
    let f = Fixture::new("remediation-postcommit").await?;
    let durable = f.split_event_store().await?;
    let fault = Arc::new(PostCommitFault {
        inner: durable.clone(),
        fail_once: AtomicBool::new(true),
    });
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&f.context, fault, calls.clone(), None)?;
    let _worker = f.command_worker(processor).await?;
    let remote = CommandBus::new(f.context.clone(), f.adapter.clone());
    let command = request("uncertain-operation", "target", "create")?;
    let encoded = remote.encode(command.clone())?;
    let error = remote.dispatch(command).await.unwrap_err();
    assert_eq!(
        error.execution_failure(),
        Some(ExecutionFailure::Store(StoreFailureKind::Unavailable))
    );
    let receipt = durable
        .load_transaction_receipt_in_context(
            f.context.name(),
            &OperationId::new("uncertain-operation")?,
        )
        .await?
        .ok_or("actual durable acceptance is missing")?;
    assert_eq!(receipt.events().len(), 1);
    assert_eq!(durable.load(&stream("target")?).await?.len(), 1);
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    tokio::time::timeout(WAIT, async {
        loop {
            let response = reader
                .read_command_response(
                    &encoded.response_address()?,
                    encoded.operation_id(),
                    encoded.message_id(),
                    Duration::from_secs(2),
                )
                .await?;
            if response.outcome() == &CommandResponseOutcome::Accepted {
                return TestResult::Ok(());
            }
            assert_eq!(
                response.outcome(),
                &CommandResponseOutcome::ExecutionFailed(ExecutionFailure::Store(
                    StoreFailureKind::Unavailable
                ))
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(durable.load(&stream("target")?).await?.len(), 1);
    assert!(f.quarantine().await?.is_empty());
    Ok(())
}

rostfrei_runtime::install_macro_support!();

#[tokio::test]
async fn response_sink_outage_exhausts_small_budget_into_durable_quarantine_without_response()
-> TestResult {
    let f = Fixture::new("remediation-exhaustion").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(InMemoryEventStore::new());
    let processor = processor(&f.context, store.clone(), calls.clone(), None)?;
    let config = ConsumerConfig::new(
        f.context.consumer_name("act", 1)?,
        f.context.durable_name("act", 1)?,
        f.context.command_address("act")?,
        Duration::from_secs(5),
        Duration::from_secs(1),
        1,
        2,
    )?;
    provision_durable_consumer(&f.js, f.messaging.topology(), &config).await?;
    let factory = NatsConsumerFactory::new(f.js.clone(), f.messaging.topology().clone());
    let consumer = <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(
        &factory,
        config.clone(),
    )?;
    let handler = Arc::new(f.adapter.command_handler(processor));
    let _worker = Worker(tokio::spawn(async move { consumer.run(handler).await }));
    let source =
        f.js.get_stream(f.messaging.topology().command_stream().as_str())
            .await?;
    let mut observer: async_nats::jetstream::consumer::PullConsumer =
        source.get_consumer(config.durable_name().as_str()).await?;
    f.js.delete_stream(f.messaging.topology().command_response_stream().as_str())
        .await?;
    let remote = CommandBus::new(f.context.clone(), f.adapter.clone());
    let command = request("budget-operation", "target", "create")?;
    let encoded = remote.encode(command.clone())?;
    let error = remote.dispatch(command).await.unwrap_err();
    assert_eq!(
        error.kind(),
        CommandBusErrorKind::Timeout,
        "no typed response can exist in an unavailable response sink"
    );
    tokio::time::timeout(WAIT, async {
        loop {
            let info = observer.info().await?;
            if info.num_ack_pending == 0 && f.quarantine().await?.len() == 1 {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    let records = f.quarantine().await?;
    let record = records.first().ok_or("missing exhaustion evidence")?;
    assert_eq!(
        record.failure_kind(),
        Some(rostfrei_messaging_core::QuarantineFailureKind::DeliveryAttemptsExhausted)
    );
    assert_eq!(record.attempt(), 2);
    assert_eq!(record.message_id(), encoded.message_id().as_str());
    assert_eq!(
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            record.payload_base64()
        )?,
        encoded.payload()
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "response reconciliation refused execution before the handler"
    );
    assert!(
        store
            .load_transaction_receipt_in_context(
                f.context.name(),
                &OperationId::new("budget-operation")?
            )
            .await?
            .is_none()
    );
    assert!(store.load(&stream("target")?).await?.is_empty());
    Ok(())
}
