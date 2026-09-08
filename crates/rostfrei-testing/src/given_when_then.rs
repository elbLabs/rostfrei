use rostfrei_core::{Aggregate, AggregateInstance};

pub fn given<A, Events>(stream_id: &rostfrei_core::StreamId, events: Events) -> Given<A>
where
    A: Aggregate,
    Events: IntoIterator<Item = A::Event>,
{
    Given {
        aggregate: AggregateInstance::rehydrate(stream_id.clone(), events),
    }
}

/// A focused aggregate harness for replaying history and exercising aggregate behavior.
///
/// Command handlers belong to the application boundary and should be tested through an
/// [`rostfrei_core::CommandExecutor`] and a concrete handler object. This harness intentionally keeps
/// persistence and command execution out of aggregate tests.
pub struct Given<A: Aggregate> {
    aggregate: AggregateInstance<A>,
}

impl<A: Aggregate> Given<A> {
    pub const fn state(&self) -> &A::State {
        self.aggregate.state()
    }

    pub fn when<Action>(self, action: Action) -> Then<A>
    where
        Action: FnOnce(&mut AggregateInstance<A>),
    {
        let mut aggregate = self.aggregate;
        action(&mut aggregate);
        let (_, state, events) = aggregate.into_parts();
        Then { state, events }
    }
}

pub type ThenParts<A> = (<A as Aggregate>::State, Vec<<A as Aggregate>::Event>);

pub struct Then<A: Aggregate> {
    state: A::State,
    events: Vec<A::Event>,
}

impl<A: Aggregate> Then<A> {
    pub const fn state(&self) -> &A::State {
        &self.state
    }

    pub fn events(&self) -> &[A::Event] {
        &self.events
    }

    pub fn into_parts(self) -> ThenParts<A> {
        (self.state, self.events)
    }
}
