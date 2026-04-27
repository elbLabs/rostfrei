use rostfrei::Aggregate;

use super::{
    RentalFleet, RentalFleetImported,
    add_bicycle::{AddBicycleActionContract, BicycleAdded},
    assess_rental_eligibility::RentalEligibilityDecisions,
    fleet_consistency::FleetConsistency,
    import_rental_fleet::ImportRentalFleetActionContract,
    rent_bicycle::{BicycleRented, RentBicycleActionContract},
    return_bicycle::{BicycleReturned, ReturnBicycleActionContract},
};
use crate::domain::BikeRental;

#[derive(Aggregate)]
#[domain(
    id = "rental-fleet",
    label = "Rental fleet",
    context = BikeRental,
    root = RentalFleet,
    actions = [
        ImportRentalFleetActionContract,
        AddBicycleActionContract,
        RentBicycleActionContract,
        ReturnBicycleActionContract
    ],
    decisions = [RentalEligibilityDecisions],
    invariants = [FleetConsistency],
    events = [RentalFleetImported, BicycleAdded, BicycleRented, BicycleReturned]
)]
pub struct RentalFleetAggregate;
