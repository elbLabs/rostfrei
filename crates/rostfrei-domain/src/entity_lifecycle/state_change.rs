#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateChange<State> {
    pub from: State,
    pub to: State,
}

impl<State> StateChange<State> {
    pub const fn new(from: State, to: State) -> Self {
        Self { from, to }
    }
}
