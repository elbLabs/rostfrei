#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateTransitionEdge<State> {
    pub from: State,
    pub to: State,
}
