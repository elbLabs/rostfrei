use rostfrei::AggregateInstance;
use uuid::Uuid;

use super::{AddBicycleAction, BicycleAdded};
use crate::domain::rental_fleet::{BicycleCondition, BicycleId, RentalFleet, RentalFleetAggregate};

impl AddBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn add_bicycle(&mut self) {
        let fleet_id = self.state().fleet_id.clone();
        let bicycle_id = allocate_bicycle_id(self.state());
        self.raise(BicycleAdded {
            fleet_id,
            bicycle_id,
            condition: BicycleCondition::Serviceable,
        });
    }
}

fn allocate_bicycle_id(root: &RentalFleet) -> BicycleId {
    let mut sequence = root.bicycles.len();
    loop {
        let seed = format!(
            "rostfrei:bike-rental:bicycle:v1:{}:{sequence}",
            root.fleet_id.as_str()
        );
        let candidate =
            BicycleId::new(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string());
        if let Some(candidate) = candidate
            && root
                .bicycles
                .iter()
                .all(|bicycle| bicycle.bicycle_id() != &candidate)
        {
            return candidate;
        }
        sequence = sequence.saturating_add(1);
    }
}
