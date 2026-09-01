use rostfrei::InvariantOwnerType;

use crate::domain::rental_fleet::{
    Bicycle, BicycleCondition, BicycleId, BicycleStatus, FleetId, RentalFleet, RentalFleetAggregate,
};

#[test]
fn rejects_duplicate_bicycle_identities() {
    let bicycle_id = BicycleId::new("bike-42").expect("fixture bicycle ID should be valid");
    let fleet = RentalFleet::new(
        FleetId::new("city-fleet").expect("fixture fleet ID should be valid"),
        vec![
            Bicycle::new(
                bicycle_id.clone(),
                BicycleStatus::Available,
                BicycleCondition::Serviceable,
            ),
            Bicycle::new(
                bicycle_id,
                BicycleStatus::Rented,
                BicycleCondition::Serviceable,
            ),
        ],
    );

    let violations = <RentalFleetAggregate as InvariantOwnerType>::validate_invariants(&fleet)
        .expect_err("duplicate bicycle identities should violate fleet consistency");

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, "bicycles");
    assert_eq!(violations[0].reason, "bicycle identities must be unique");
}
