use bike_rental::rental::{
    BicycleAvailability, BicycleCondition, BicycleId, BicycleRented, BicycleStatus,
    BicycleUnavailable, FleetId, ImportedBicycle, RentalFleetActions, RentalFleetAggregate,
    RentalFleetImported,
};
use rostfrei::{AggregateInstance, EventVariant, StreamAggregateId, StreamAggregateType, StreamId};

fn fleet(
    status: BicycleStatus,
    condition: BicycleCondition,
) -> AggregateInstance<RentalFleetAggregate> {
    let fleet_id = FleetId::new("city-fleet").unwrap();
    AggregateInstance::rehydrate(
        StreamId::new(
            StreamAggregateType::new("bike-rental/rental-fleet").unwrap(),
            StreamAggregateId::new(fleet_id.as_str()).unwrap(),
        ),
        [RentalFleetImported {
            fleet_id,
            bicycles: vec![ImportedBicycle {
                bicycle_id: BicycleId::new("bike-42").unwrap(),
                status,
                condition,
            }],
        }
        .into()],
    )
}

#[test]
fn rents_an_available_serviceable_bicycle() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let bicycle_id = BicycleId::new("bike-42").unwrap();

    fleet.rent_bicycle(bicycle_id.clone()).unwrap();

    let event = EventVariant::<BicycleRented>::event(&fleet.uncommitted_events()[0]).unwrap();
    assert_eq!(event.bicycle_id, bicycle_id);
    assert_eq!(fleet.state().bicycles()[0].status(), BicycleStatus::Rented);
    assert_eq!(
        RentalFleetAggregate::bicycle_availability(fleet.state(), &event.bicycle_id),
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

    let error = fleet.rent_bicycle(bicycle_id.clone()).unwrap_err();

    assert_eq!(error, BicycleUnavailable { bicycle_id });
    assert!(fleet.uncommitted_events().is_empty());
    assert_eq!(
        fleet.state().bicycles()[0].status(),
        BicycleStatus::Available
    );
}
