use rostfrei::StateTransition;

use super::BicycleStatus;

#[derive(StateTransition, Clone, Copy, Debug, Eq, PartialEq)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[edge(
        id = "rent",
        label = "Rent",
        from = Available,
        to = Rented
    )]
    Rent,
    #[edge(
        id = "return",
        label = "Return",
        from = Rented,
        to = Available
    )]
    Return,
}
