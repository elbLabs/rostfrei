use rostfrei::{AggregateInstance, Apply, Command, CommandHandler, CommandType, DomainEvent};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    Bicycle, BicycleCondition, BicycleId, BicycleStatus, FleetId, RentalFleet, RentalFleetActions,
    RentalFleetAggregate,
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "add-bicycle",
    label = "Add bicycle",
    owner = RentalFleetAggregate,
    json,
    runtime
)]
pub struct AddBicycle;

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-added", label = "Bicycle added")]
pub struct BicycleAdded {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
    #[domain(value_object)]
    pub condition: BicycleCondition,
}

pub(super) fn allocate_bicycle_id(root: &RentalFleet) -> BicycleId {
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

impl CommandHandler<AddBicycle> for RentalFleetAggregate {
    type Rejection = <AddBicycle as CommandType>::Rejection;

    fn handle(
        _command: &AddBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.add_bicycle();
        Ok(())
    }
}

impl Apply<BicycleAdded> for RentalFleet {
    fn apply(&mut self, event: &BicycleAdded) {
        self.bicycles.push(Bicycle::new(
            event.bicycle_id.clone(),
            BicycleStatus::Available,
            event.condition,
        ));
    }
}
