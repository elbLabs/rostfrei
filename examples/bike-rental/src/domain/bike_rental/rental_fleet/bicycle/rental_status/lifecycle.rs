use rostfrei::EntityLifecycle;

use super::super::{Bicycle, mark_available::MarkAvailableAction, mark_rented::MarkRentedAction};

#[derive(EntityLifecycle)]
#[domain(
    id = "rental-status",
    label = "Bicycle rental status",
    owner = Bicycle,
    initial = Available
)]
pub enum BicycleRentalLifecycle {
    #[domain(id = "available", label = "Available")]
    #[transition(action = MarkRentedAction::MARK_RENTED, to = Rented)]
    Available,
    #[domain(id = "rented", label = "Rented")]
    #[transition(action = MarkAvailableAction::MARK_AVAILABLE, to = Available)]
    Rented,
}
