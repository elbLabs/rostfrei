use rostfrei::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "rental-status", label = "Bicycle rental status")]
pub enum BicycleRentalLifecycle {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
