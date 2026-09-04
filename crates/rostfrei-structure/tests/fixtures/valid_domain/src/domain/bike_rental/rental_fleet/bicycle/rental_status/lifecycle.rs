#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "bicycle-rental-status", label = "Bicycle rental status")]
#[lifecycle(initial = Available)]
pub enum BicycleStatus {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
