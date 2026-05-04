use rostfrei::StateTransition;

use super::BicycleStatus;

#[derive(StateTransition, Clone, Copy, Debug, Eq, PartialEq)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[transition(id = "rent", label = "Rent")]
    #[edge(from = Available, to = Rented)]
    Rent,
    #[transition(id = "return", label = "Return")]
    #[edge(from = Rented, to = Available)]
    Return,
    #[transition(id = "retire", label = "Retire")]
    #[edge(from = Available, to = Retired)]
    #[edge(from = Rented, to = Retired)]
    Retire,
}
