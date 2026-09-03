#[derive(Entity)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    bicycle_id: BicycleId,
    status: BicycleStatus,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        &self.bicycle_id
    }
}
