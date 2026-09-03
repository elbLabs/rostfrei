use crate::domain::bike_rental::rental_fleet::fleet_consistency::FleetConsistency;
use crate::domain::rental_fleet::{
    Bicycle, BicycleCondition, BicycleId, BicycleStatus, FleetId, RentalFleet, RentalFleetAggregate,
};

#[rostfrei::domain_invariant_test(<RentalFleetAggregate as FleetConsistency>::DESCRIPTOR)]
fn exposes_owner_independent_invariant_metadata() {
    let descriptor = <RentalFleetAggregate as FleetConsistency>::DESCRIPTOR;

    assert_eq!(
        descriptor.id,
        rostfrei::InvariantId("unique-bicycle-identities")
    );
    assert_eq!(descriptor.label, "Bicycle identities are unique");
}

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

    let violation = <RentalFleetAggregate as FleetConsistency>::unique_bicycle_identities(&fleet)
        .expect("duplicate bicycle identities should violate fleet consistency");

    assert_eq!(violation.path, "bicycles");
    assert_eq!(violation.reason, "bicycle identities must be unique");
}
