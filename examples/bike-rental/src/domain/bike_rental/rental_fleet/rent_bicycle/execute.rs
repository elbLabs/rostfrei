use rostfrei::AggregateInstance;

use super::{BicycleRented, BicycleUnavailable, RentBicycleAction};
use crate::domain::bike_rental::rental_fleet::{
    BicycleId, RentalFleetAggregate,
    assess_rental_eligibility::{RentalEligibilityOutcome, RentalEligibilityPolicy as _},
};

impl RentBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable> {
        let event = {
            let root = self.state();
            let bicycle = root
                .bicycles
                .iter()
                .find(|bicycle| bicycle.bicycle_id() == &input)
                .ok_or_else(|| BicycleUnavailable {
                    bicycle_id: input.clone(),
                })?;
            match bicycle.assess_rental_eligibility() {
                RentalEligibilityOutcome::Eligible => BicycleRented {
                    fleet_id: root.fleet_id.clone(),
                    bicycle_id: input,
                },
                RentalEligibilityOutcome::UnavailableStatus
                | RentalEligibilityOutcome::MaintenanceRequired => {
                    return Err(BicycleUnavailable { bicycle_id: input });
                }
            }
        };

        self.raise(event);
        Ok(())
    }
}
