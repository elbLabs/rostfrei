#[derive(Command)]
#[domain(context = BikeRental, id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}
