use rostfrei::{Entity, EntityDefinition};

use super::{Bicycle, FleetId, RentalFleetAggregate};

#[derive(Entity, Debug)]
#[domain(id = "rental-fleet-root", label = "Rental fleet")]
pub struct RentalFleet {
    #[domain(identity)]
    pub(super) fleet_id: FleetId,
    #[domain(entity)]
    pub(super) bicycles: Vec<Bicycle>,
}

impl EntityDefinition for RentalFleet {
    type Owner = RentalFleetAggregate;
    type Identity = FleetId;
}

impl RentalFleet {
    pub const fn new(fleet_id: FleetId, bicycles: Vec<Bicycle>) -> Self {
        Self { fleet_id, bicycles }
    }

    pub fn bicycles(&self) -> &[Bicycle] {
        &self.bicycles
    }

    pub const fn fleet_id(&self) -> &FleetId {
        &self.fleet_id
    }
}
