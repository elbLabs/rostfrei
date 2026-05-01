#[derive(EntityLifecycle)]
#[domain(id = "bicycle-rental-status", label = "Bicycle rental status")]
pub enum BicycleRentalStatus {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
