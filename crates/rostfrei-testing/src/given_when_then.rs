use rostfrei_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};

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

    pub fn when<Command>(
        self,
        command: &Command,
    ) -> Then<A, <A as CommandHandler<Command>>::Rejection>
    where
        A: CommandHandler<Command>,
    {
        let mut aggregate = self.aggregate;
        let decision = A::handle(command, &mut aggregate);
        let (_, state, events) = aggregate.into_parts();
        Then {
            state,
            events,
            decision,
        }
    }
}

pub struct Then<A: Aggregate, Rejection> {
    state: A::State,
    events: Vec<A::Event>,
    decision: Result<(), Rejection>,
}

impl<A: Aggregate, Rejection> Then<A, Rejection> {
    pub const fn state(&self) -> &A::State {
        &self.state
    }

    pub fn events(&self) -> &[A::Event] {
        &self.events
    }

    pub const fn decision(&self) -> Result<(), &Rejection> {
        match &self.decision {
            Ok(()) => Ok(()),
            Err(rejection) => Err(rejection),
        }
    }

    pub const fn is_accepted(&self) -> bool {
        self.decision.is_ok()
    }

    pub fn into_parts(self) -> (A::State, Vec<A::Event>, Result<(), Rejection>) {
        (self.state, self.events, self.decision)
    }
}
