#[derive(StateTransition)]
#[transition(state = RentalFleetState)]
pub enum RentalFleetTransition {
    #[transition(id = "open", label = "Open")]
    #[edge(from = Closed, to = Open)]
    Open,
}
