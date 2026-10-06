//! Executor acceptance scenarios shared by memory and durable event stores.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandDecision, CommandExecution,
    CommandExecutionError, CommandExecutionMetadata, CommandExecutor, CommandHandler,
    CommandHandlingResult, CommandOutcome, CommandReceipt, ContentFingerprint, Event,
    EventCodecError, EventStore, EventStoreErrorKind, OperationId, RecordedEvent, StreamId,
};
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};
use tokio::sync::Barrier;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct Customer;

impl Aggregate for Customer {
    type State = bool;
    type Event = Registered;
    const BOUNDED_CONTEXT: &'static str = "acceptance";
    const AGGREGATE_TYPE: &'static str = "customer";

    fn initial(_stream_id: &StreamId) -> bool {
        false
    }
    fn apply(state: &mut bool, _event: &Registered) {
        *state = true;
    }
}

struct Registered;

impl Event for Registered {
    fn event_type(&self) -> &'static str {
        "registered"
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        Ok(b"{}".to_vec())
    }
    fn decode_json(_event: &RecordedEvent) -> Result<Self, EventCodecError> {
        Ok(Self)
    }
}

#[derive(Default)]
struct EnsureCustomer {
    calls: AtomicUsize,
    first_attempts: Option<Arc<Barrier>>,
}

struct RegisterCustomer {
    phone: Option<&'static str>,
}

impl RegisterCustomer {
    const fn for_phone(phone: &'static str) -> Self {
        Self { phone: Some(phone) }
    }
}

#[async_trait]
impl CommandHandler<RegisterCustomer> for EnsureCustomer {
    type Rejection = ();

    async fn handle(
        &self,
        command: &RegisterCustomer,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<()> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        if let Some(phone) = command.phone {
            let mut customer = execution.load::<Customer>(phone).await?;
            if call < 2
                && let Some(barrier) = &self.first_attempts
            {
                let _ = barrier.wait().await;
            }
            if !customer.aggregate().state() {
                customer.aggregate_mut().raise(Registered);
            }
        }
        Ok(CommandDecision::Accepted)
    }
}

fn metadata(
    operation: &str,
    evidence: &str,
) -> Result<CommandExecutionMetadata, Box<dyn std::error::Error + Send + Sync>> {
    Ok(CommandExecutionMetadata::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(evidence),
    )
    .with_bounded_context(BoundedContextName::new("acceptance")?)
    .with_correlation_id(CorrelationId::new("original-correlation")?)
    .with_causation_id(CausationId::new("original-causation")?))
}

/// Runs event-free replay and concurrent first-registration scenarios in the `acceptance` context.
pub async fn run(store: Arc<dyn EventStore>) -> TestResult {
    replay_and_changed_evidence(Arc::clone(&store)).await?;
    losing_first_registration(Arc::clone(&store)).await?;
    simultaneous_same_operation(store).await
}

async fn replay_and_changed_evidence(store: Arc<dyn EventStore>) -> TestResult {
    let handler = EnsureCustomer::default();
    let executor = CommandExecutor::new(Arc::clone(&store));
    executor
        .execute(
            &handler,
            metadata("register-p", "phone-p")?,
            &RegisterCustomer::for_phone("phone-p"),
        )
        .await?;
    for (operation, phone) in [("known-p", Some("phone-p")), ("empty", None)] {
        let evidence = phone.unwrap_or("empty");
        let command = RegisterCustomer { phone };
        let accepted = executor
            .execute(&handler, metadata(operation, evidence)?, &command)
            .await?;
        assert_eq!(
            accepted,
            CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
        );
        let recreated_handler = EnsureCustomer::default();
        let recreated = CommandExecutor::new(Arc::clone(&store));
        let replay = recreated
            .execute(&recreated_handler, metadata(operation, evidence)?, &command)
            .await?;
        assert_eq!(
            replay,
            CommandOutcome::Accepted(CommandReceipt::ExactReplay(Vec::new()))
        );
        for changed in [
            metadata(operation, "phone-q")?,
            metadata(operation, evidence)?.with_correlation_id(CorrelationId::new("changed")?),
            metadata(operation, evidence)?.with_causation_id(CausationId::new("changed")?),
        ] {
            assert!(
                matches!(recreated.execute(&recreated_handler, changed, &RegisterCustomer::for_phone("phone-q")).await,
                Err(CommandExecutionError::Store(error)) if error.kind() == EventStoreErrorKind::IdentityConflict)
            );
        }
        assert_eq!(recreated_handler.calls.load(Ordering::Relaxed), 0);
    }
    assert!(
        store
            .load(&StreamId::new(
                AggregateType::new("customer")?,
                AggregateId::new("phone-q")?
            ))
            .await?
            .is_empty()
    );
    Ok(())
}

async fn losing_first_registration(store: Arc<dyn EventStore>) -> TestResult {
    let handler = EnsureCustomer {
        calls: AtomicUsize::new(0),
        first_attempts: Some(Arc::new(Barrier::new(2))),
    };
    let executor = CommandExecutor::new(Arc::clone(&store));
    let first_metadata = metadata("first-registration-a", "race-phone")?;
    let second_metadata = metadata("first-registration-b", "race-phone")?;
    let command = RegisterCustomer::for_phone("race-phone");
    let results = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(
            executor.execute(&handler, first_metadata, &command),
            executor.execute(&handler, second_metadata, &command),
        )
    })
    .await?;
    let outcomes: [_; 2] = results.into();
    let outcomes = outcomes.into_iter().collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                CommandOutcome::Accepted(CommandReceipt::Appended(_))
            ))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)
            ))
            .count(),
        1
    );
    assert_eq!(handler.calls.load(Ordering::Relaxed), 3);
    for (operation, outcome) in ["first-registration-a", "first-registration-b"]
        .into_iter()
        .zip(outcomes)
    {
        let CommandOutcome::Accepted(receipt) = outcome else {
            return Err("registration was rejected".into());
        };
        let original = metadata(operation, "race-phone")?;
        let persisted = store
            .load_transaction_receipt_in_context(
                &BoundedContextName::new("acceptance")?,
                original.operation_id(),
            )
            .await?
            .ok_or("missing registration acceptance")?;
        assert_eq!(persisted.events(), receipt.events());
        let replay = executor
            .execute(
                &handler,
                original,
                &RegisterCustomer::for_phone("race-phone"),
            )
            .await?;
        assert_eq!(
            replay,
            CommandOutcome::Accepted(CommandReceipt::ExactReplay(receipt.events().to_vec()))
        );
        assert!(
            matches!(executor.execute(&handler, metadata(operation, "changed-phone")?, &RegisterCustomer::for_phone("changed-phone")).await,
            Err(CommandExecutionError::Store(error)) if error.kind() == EventStoreErrorKind::IdentityConflict)
        );
    }
    assert_eq!(handler.calls.load(Ordering::Relaxed), 3);
    Ok(())
}

async fn simultaneous_same_operation(store: Arc<dyn EventStore>) -> TestResult {
    let executor = CommandExecutor::new(store);
    executor
        .execute(
            &EnsureCustomer::default(),
            metadata("same-op-seed", "same-op-phone")?,
            &RegisterCustomer::for_phone("same-op-phone"),
        )
        .await?;
    for changed in [false, true] {
        let operation = if changed {
            "changed-evidence-race"
        } else {
            "identical-evidence-race"
        };
        let handler = EnsureCustomer {
            calls: AtomicUsize::new(0),
            first_attempts: Some(Arc::new(Barrier::new(2))),
        };
        let first = metadata(operation, "same-op-phone")?;
        let second = metadata(operation, if changed { "changed" } else { "same-op-phone" })?;
        let command = RegisterCustomer::for_phone("same-op-phone");
        let results: [_; 2] = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(
                executor.execute(&handler, first, &command),
                executor.execute(&handler, second, &command)
            )
        })
        .await?
        .into();
        if changed {
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert!(results.iter().any(|result| matches!(result, Err(CommandExecutionError::Store(error)) if error.kind() == EventStoreErrorKind::IdentityConflict)));
        } else {
            let outcomes = results.into_iter().collect::<Result<Vec<_>, _>>()?;
            assert!(outcomes.contains(&CommandOutcome::Accepted(CommandReceipt::AcceptedNoEvents)));
            assert!(
                outcomes.contains(&CommandOutcome::Accepted(CommandReceipt::ExactReplay(
                    Vec::new()
                )))
            );
        }
    }
    Ok(())
}
