#[derive(Entity)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle;

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        todo!()
    }
}
