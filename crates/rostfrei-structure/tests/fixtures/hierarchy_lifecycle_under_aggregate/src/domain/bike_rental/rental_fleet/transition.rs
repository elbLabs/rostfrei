#[derive(StateTransition)]
#[transition(state = RentalFleetState)]
pub enum RentalFleetTransition {
    #[edge(id = "open", label = "Open", from = Closed, to = Open)]
    Open,
}
