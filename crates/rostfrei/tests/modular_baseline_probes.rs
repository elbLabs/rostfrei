//! Runnable against 050f086. These are behavioral REDs, not missing-API diagnostics.
#![allow(clippy::panic_in_result_fn)]
mod modular_support;
use async_trait::async_trait;
use modular_support::*;
rostfrei::install_macro_support!();

/// A real authority with a controllable read outage; commits/sessions are forwarded intact.
struct AuthorityGate {
    inner: InMemoryEventStore,
    down: Arc<AtomicBool>,
}
#[async_trait]
impl EventHistory for AuthorityGate {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.inner.load(id).await
    }
}
#[async_trait]
impl EventStore for AuthorityGate {
    async fn load_completed_outcome_in_context(
        &self,
        c: &rostfrei_messaging_core::BoundedContextName,
        id: &OperationId,
    ) -> Result<Option<CompletedOutcome>, EventStoreError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::Unavailable,
                "authority deliberately unavailable",
            ));
        }
        self.inner.load_completed_outcome_in_context(c, id).await
    }
    async fn append_session(&self) -> Result<Box<dyn AppendSession + '_>, EventStoreError> {
        self.inner.append_session().await
    }
    async fn commit_outcome(
        &self,
        t: EventTransaction,
    ) -> Result<CompletedAppendOutcome, EventStoreError> {
        self.inner.commit_outcome(t).await
    }
    async fn append(
        &self,
        id: &StreamId,
        v: ExpectedVersion,
        b: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.inner.append(id, v, b).await
    }
    async fn load_transaction_receipt(
        &self,
        id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner.load_transaction_receipt(id).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        c: &rostfrei_messaging_core::BoundedContextName,
        id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner.load_transaction_receipt_in_context(c, id).await
    }
    async fn append_transaction(
        &self,
        t: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.inner.append_transaction(t).await
    }
}

#[tokio::test]
async fn cached_local_response_must_reconsult_authority() -> TestResult {
    let down = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = baseline_processor(
        Arc::new(AuthorityGate {
            inner: InMemoryEventStore::new(),
            down: down.clone(),
        }),
        calls.clone(),
    )?;
    let bus = bus(processor.clone())?;
    let encoded = bus.encode(request("cached-operation", true)?)?;
    assert!(matches!(
        bus.dispatch_encoded(encoded.clone())
            .await?
            .response()
            .outcome(),
        CommandResponseOutcome::Rejected(_)
    ));
    down.store(true, Ordering::SeqCst);
    assert!(
        processor.process(&encoded).await.is_err(),
        "positive control: authority fault reaches direct processing"
    );
    let cached = bus.dispatch_encoded(encoded).await;
    assert!(
        cached.is_err(),
        "LIFE-4/AUTH-2 prerequisite: a local projection returned a business result while authority was unavailable: {cached:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
#[allow(
    clippy::let_unit_value,
    clippy::ignored_unit_patterns,
    reason = "one behavioral probe must compile with both baseline unit and future Result registration APIs"
)]
async fn duplicate_codec_registration_must_not_silently_change_historical_meaning() -> TestResult {
    let store = InMemoryEventStore::new();
    seed_v1(&store).await?;
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let mut codecs = AggregateCodecs::default();
    // Deliberately discard the registration result: both old unit-returning and new fallible
    // signatures compile. The future contract target also asserts the typed error itself.
    let _ = codecs.register::<Counter, _>(CentsCodec {
        scale: 100,
        decodes: first.clone(),
    });
    let _ = codecs.register::<Counter, _>(CentsCodec {
        scale: 1,
        decodes: second.clone(),
    });
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("duplicate-codec")?,
        ContentFingerprint::digest("same"),
    )
    .with_bounded_context(context()?.name().clone());
    let simulation = CommandExecutor::with_codecs(store, Arc::new(codecs))
        .simulate(
            &ChangeHandler(Arc::new(AtomicUsize::new(0))),
            metadata,
            &Change {
                amount: 0,
                reject: true,
            },
        )
        .await?;
    assert_eq!(
        simulation.into_parts().0,
        CommandDecision::Rejected("balance=200".to_owned()),
        "CODEC-4: second same-Rust-type instance silently replaced the registered policy"
    );
    assert_eq!(first.load(Ordering::SeqCst), 1);
    assert_eq!(second.load(Ordering::SeqCst), 0);
    Ok(())
}
