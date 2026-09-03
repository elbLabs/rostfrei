#[derive(DomainEvent)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    pub bicycle_id: BicycleId,
}
