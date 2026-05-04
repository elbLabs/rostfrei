#[derive(StateTransition)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[edge(
        id = "rent",
        label = "Rent",
        from = Available,
        to = Rented
    )]
    Rent,
}
