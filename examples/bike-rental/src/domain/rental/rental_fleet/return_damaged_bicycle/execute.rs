use rostfrei::AggregateInstance;

use super::{BicycleDamageReported, ReturnDamagedBicycleAction};
use crate::domain::rental_fleet::{
    BicycleId, BicycleNotRented, RentalFleetAggregate, ReturnBicycleAction as _,
};

impl ReturnDamagedBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn return_damaged_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        damage_report: String,
    ) -> Result<(), BicycleNotRented> {
        self.return_bicycle(bicycle_id.clone())?;
        self.raise(BicycleDamageReported {
            fleet_id: self.state().fleet_id.clone(),
            bicycle_id,
            damage_report,
        });
        Ok(())
    }
}
