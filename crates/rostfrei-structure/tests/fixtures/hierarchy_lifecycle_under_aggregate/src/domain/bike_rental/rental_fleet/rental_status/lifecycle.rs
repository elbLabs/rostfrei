#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "rental-status", label = "Rental status")]
#[lifecycle(initial = Available)]
pub enum RentalStatus {
    #[state(id = "available", label = "Available")]
    Available,
}
