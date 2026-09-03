#[derive(Command)]
#[domain(id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle {
    pub bicycle_id: BicycleId,
}
