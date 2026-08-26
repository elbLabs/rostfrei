use rostfrei_core::{Aggregate, CommandHandler, DecisionContext};

pub fn given<A, Events>(events: Events) -> Given<A>
where
    A: Aggregate,
    Events: IntoIterator<Item = A::Event>,
{
    let mut state = A::initial();
    for event in events {
        state.apply(&event);
    }
    Given { state }
}

pub struct Given<A: Aggregate> {
    state: A,
}

impl<A: Aggregate> Given<A> {
    pub fn state(&self) -> &A {
        &self.state
    }

    pub fn when<Command>(
        mut self,
        command: &Command,
    ) -> Then<A, <A as CommandHandler<Command>>::Rejection>
    where
        A: CommandHandler<Command>,
    {
        let mut events = Vec::new();
        let mut context = DecisionContext::new(&mut self.state, &mut events);
        let decision = A::handle(command, &mut context);
        Then {
            state: self.state,
            events,
            decision,
        }
    }
}

pub struct Then<A: Aggregate, Rejection> {
    state: A,
    events: Vec<A::Event>,
    decision: Result<(), Rejection>,
}

impl<A: Aggregate, Rejection> Then<A, Rejection> {
    pub fn state(&self) -> &A {
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

    pub fn into_parts(self) -> (A, Vec<A::Event>, Result<(), Rejection>) {
        (self.state, self.events, self.decision)
    }
}
