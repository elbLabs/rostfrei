#[derive(Entity)]
#[domain(id = "rental-fleet", label = "Rental fleet")]
pub struct RentalFleet;

pub struct AnotherRoot;

impl EntityDefinition for AnotherRoot {
    type Owner = RentalFleetAggregate;
    type Identity = FleetId;

    fn identity(&self) -> &Self::Identity {
        todo!()
    }
}
