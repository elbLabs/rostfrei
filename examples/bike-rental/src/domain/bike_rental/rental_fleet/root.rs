use rostfrei::{Entity, EntityDefinition};
use serde::{Deserialize, Serialize};

use super::{Bicycle, FleetId, RentalFleetAggregate};

#[derive(Entity, Debug, Deserialize, Serialize)]
#[domain(id = "rental-fleet-root", label = "Rental fleet")]
pub struct RentalFleet {
    pub(super) fleet_id: FleetId,
    #[domain(entity)]
    pub(super) bicycles: Vec<Bicycle>,
}

impl EntityDefinition for RentalFleet {
    type Owner = RentalFleetAggregate;
    type Identity = FleetId;

    fn identity(&self) -> &Self::Identity {
        &self.fleet_id
    }
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
