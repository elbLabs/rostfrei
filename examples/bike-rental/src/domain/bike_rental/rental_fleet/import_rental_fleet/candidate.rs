use super::ImportedBicycle;
use crate::domain::rental_fleet::{Bicycle, FleetId, RentalFleet};

pub(super) fn imported_fleet(fleet_id: FleetId, bicycles: &[ImportedBicycle]) -> RentalFleet {
    RentalFleet::new(
        fleet_id,
        bicycles
            .iter()
            .map(|bicycle| {
                Bicycle::new(
                    bicycle.bicycle_id.clone(),
                    bicycle.status,
                    bicycle.condition,
                )
            })
            .collect(),
    )
}
