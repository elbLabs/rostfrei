use super::{ReservedBicycleReturned, ReturnCollectedBicycleAction};
use crate::domain::rental_fleet::{
    BicycleCondition, BicycleId, BicycleNotRented, RentalFleetAggregate, RentalReturn,
    ReturnBicycleAction as _, ReturnDamagedBicycleAction as _, ReturnTime,
};
use rostfrei::AggregateInstance;

impl ReturnCollectedBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn return_collected_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        returned_at: Option<ReturnTime>,
        damage_report: Option<String>,
    ) -> Result<(), BicycleNotRented> {
        let reservation = self
            .state()
            .active_reservation(&bicycle_id)
            .ok_or_else(|| BicycleNotRented {
                bicycle_id: bicycle_id.clone(),
            })?;
        let collection = reservation.collection().ok_or_else(|| BicycleNotRented {
            bicycle_id: bicycle_id.clone(),
        })?;
        let event = ReservedBicycleReturned {
            fleet_id: self.state().fleet_id().clone(),
            reservation_id: reservation.reservation_id().clone(),
            bicycle_id: bicycle_id.clone(),
            payment_id: collection.payment_id.clone(),
            details: RentalReturn {
                returned_at,
                damaged: damage_report.is_some()
                    || self.state().bicycles().iter().any(|bike| {
                        bike.bicycle_id() == &bicycle_id
                            && bike.condition() == BicycleCondition::MaintenanceRequired
                    }),
            },
        };
        match damage_report {
            Some(report) => self.return_damaged_bicycle(bicycle_id, report)?,
            None => self.return_bicycle(bicycle_id)?,
        }
        self.raise(event);
        Ok(())
    }
}
