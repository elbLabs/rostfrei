#[derive(StateTransition)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[transition(id = "rent", label = "Rent")]
    #[edge(from = Available, to = Rented)]
    Rent,
}
