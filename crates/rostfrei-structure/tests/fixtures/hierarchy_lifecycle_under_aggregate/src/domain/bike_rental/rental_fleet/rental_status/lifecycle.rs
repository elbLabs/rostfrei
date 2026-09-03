#[derive(EntityLifecycle)]
#[domain(id = "rental-status", label = "Rental status")]
pub enum RentalStatus {
    #[state(id = "available", label = "Available")]
    Available,
}
