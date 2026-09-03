#[derive(Entity)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    id: BicycleId,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}
