#[derive(Entity)]
#[domain(id = "rental-fleet-root", label = "Rental fleet")]
pub struct RentalFleet {
    fleet_id: FleetId,
    status: FleetStatus,
    #[domain(entity)]
    bicycles: Vec<Bicycle>,
}

impl EntityDefinition for RentalFleet {
    type Owner = RentalFleetAggregate;
    type Identity = FleetId;

    fn identity(&self) -> &Self::Identity {
        &self.fleet_id
    }
}
