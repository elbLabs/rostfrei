#[derive(StateTransition)]
#[transition(state = FleetStatus)]
pub enum FleetTransition {
    #[transition(id = "retire", label = "Retire")]
    #[edge(from = Active, to = Retired)]
    Retire,
}
