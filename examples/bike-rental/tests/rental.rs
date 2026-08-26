use bike_rental::rental::{
    Bicycle, BicycleAvailability, BicycleCondition, BicycleId, BicycleStatus, BicycleUnavailable,
    FleetId, RentBicycle, RentalFleet, RentalFleetActions, RentalFleetAggregate,
};

fn fleet(status: BicycleStatus, condition: BicycleCondition) -> RentalFleet {
    RentalFleet::new(
        FleetId::new("city-fleet").unwrap(),
        vec![Bicycle::new(
            BicycleId::new("bike-42").unwrap(),
            status,
            condition,
        )],
    )
}

#[test]
fn rents_an_available_serviceable_bicycle() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let bicycle_id = BicycleId::new("bike-42").unwrap();

    let event = RentalFleetAggregate::rent_bicycle(
        &mut fleet,
        RentBicycle {
            bicycle_id: bicycle_id.clone(),
        },
    )
    .unwrap();

    assert_eq!(event.bicycle_id, bicycle_id);
    assert_eq!(fleet.bicycles()[0].status(), BicycleStatus::Rented);
    assert_eq!(
        RentalFleetAggregate::bicycle_availability(&fleet, &event.bicycle_id),
        Some(BicycleAvailability::Unavailable)
    );
}

#[test]
fn rejects_an_unavailable_bicycle_without_changing_it() {
    let mut fleet = fleet(
        BicycleStatus::Available,
        BicycleCondition::MaintenanceRequired,
    );
    let bicycle_id = BicycleId::new("bike-42").unwrap();

    let error = RentalFleetAggregate::rent_bicycle(
        &mut fleet,
        RentBicycle {
            bicycle_id: bicycle_id.clone(),
        },
    )
    .unwrap_err();

    assert_eq!(error, BicycleUnavailable { bicycle_id });
    assert_eq!(fleet.bicycles()[0].status(), BicycleStatus::Available);
}
