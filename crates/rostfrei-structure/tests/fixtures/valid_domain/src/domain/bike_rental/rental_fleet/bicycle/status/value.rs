#[derive(ValueObject)]
#[domain(id = "bicycle-status", label = "Bicycle status")]
pub enum BicycleStatus {
    Available,
    Rented,
}
