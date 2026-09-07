use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateInstance, CommandContext, CommandDecision, CommandExecutionError,
    CommandHandler, ContentFingerprint, EventHistory, EventStoreError, ExecutionMetadata,
    OperationId, RecordedEvent, StreamId,
};

pub fn given<A, Events>(stream_id: &StreamId, events: Events) -> Given<A>
where
    A: Aggregate,
    Events: IntoIterator<Item = A::Event>,
{
    Given {
        aggregate: AggregateInstance::rehydrate(stream_id.clone(), events),
    }
}

pub struct Given<A: Aggregate> {
    aggregate: AggregateInstance<A>,
}

impl<A: Aggregate> Given<A> {
    pub const fn state(&self) -> &A::State {
        self.aggregate.state()
    }

    pub async fn when<Command>(
        self,
        command: &Command,
    ) -> Then<A, <A as CommandHandler<Command>>::Rejection>
    where
        A: CommandHandler<Command>,
        Command: Sync,
        A::State: Send,
        A::Event: Send,
    {
        let mut aggregate = self.aggregate;
        let metadata = ExecutionMetadata::new(
            aggregate.stream_id().clone(),
            harness_operation_id(),
            ContentFingerprint::digest(b"rostfrei-testing/given-when-then"),
        );
        let history = EmptyEventHistory;
        let mut context = CommandContext::new(&history, &metadata);
        let decision = A::handle(command, &mut aggregate, &mut context).await;
        let (_, state, events) = aggregate.into_parts();
        Then {
            state,
            events,
            decision,
        }
    }
}

#[allow(
    clippy::expect_used,
    reason = "the static harness operation ID is known to satisfy identifier validation"
)]
fn harness_operation_id() -> OperationId {
    OperationId::new("rostfrei-testing-given-when-then")
        .expect("the static harness operation ID must be valid")
}

struct EmptyEventHistory;

#[async_trait]
impl EventHistory for EmptyEventHistory {
    async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Ok(Vec::new())
    }
}

pub type ThenParts<A, Rejection> = (
    <A as Aggregate>::State,
    Vec<<A as Aggregate>::Event>,
    Result<CommandDecision<Rejection>, CommandExecutionError>,
);

pub struct Then<A: Aggregate, Rejection> {
    state: A::State,
    events: Vec<A::Event>,
    decision: Result<CommandDecision<Rejection>, CommandExecutionError>,
}

impl<A: Aggregate, Rejection> Then<A, Rejection> {
    pub const fn state(&self) -> &A::State {
        &self.state
    }

    pub fn events(&self) -> &[A::Event] {
        &self.events
    }

    pub const fn decision(&self) -> Result<&CommandDecision<Rejection>, &CommandExecutionError> {
        match &self.decision {
            Ok(decision) => Ok(decision),
            Err(error) => Err(error),
        }
    }

    pub const fn is_accepted(&self) -> bool {
        matches!(self.decision, Ok(CommandDecision::Accepted))
    }

    pub fn into_parts(self) -> ThenParts<A, Rejection> {
        (self.state, self.events, self.decision)
    }
}
