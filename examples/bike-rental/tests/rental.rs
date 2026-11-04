#![allow(clippy::unwrap_used, reason = "static domain fixtures")]

use bike_rental::rental_fleet::{
    self, AddBicycleAction as _, BicycleCondition, BicycleId, BicycleNotRented, BicycleStatus,
    BicycleTransfer, BicycleTransferAction as _, BicycleTransferRejectionReason, FleetId,
    RentBicycleAction as _, RentalFleetAggregate, RetireBicycleAction as _,
    ReturnBicycleAction as _, TransferBicycle,
};
use rostfrei::AggregateInstance;

fn fleet(id: &str) -> AggregateInstance<RentalFleetAggregate> {
    AggregateInstance::rehydrate(rental_fleet::stream_id(id).unwrap(), vec![])
}

#[test]
fn rental_roundtrip_rehydrates_and_refuses_a_second_return() {
    let id = BicycleId::new("bike-42").unwrap();
    let mut fleet = fleet("city-fleet");
    fleet
        .add_bicycle(id.clone(), BicycleCondition::Serviceable)
        .unwrap();
    fleet.rent_bicycle(id.clone()).unwrap();
    assert_eq!(fleet.state().bicycles()[0].status(), BicycleStatus::Rented);
    fleet.return_bicycle(id.clone()).unwrap();
    assert_eq!(
        fleet.return_bicycle(id.clone()),
        Err(BicycleNotRented { bicycle_id: id })
    );
    assert_eq!(fleet.uncommitted_events().len(), 3);
    let replay = AggregateInstance::<RentalFleetAggregate>::rehydrate(
        rental_fleet::stream_id("city-fleet").unwrap(),
        fleet.into_parts().2,
    );
    assert_eq!(
        replay.state().bicycles()[0].status(),
        BicycleStatus::Available
    );
}

#[test]
fn retired_bicycle_cannot_be_rented_or_retired_again() {
    let id = BicycleId::new("bike-42").unwrap();
    let mut fleet = fleet("city-fleet");
    fleet
        .add_bicycle(id.clone(), BicycleCondition::Serviceable)
        .unwrap();
    fleet.retire_bicycle(id.clone()).unwrap();
    assert!(fleet.rent_bicycle(id.clone()).is_err());
    assert!(fleet.retire_bicycle(id).is_err());
    assert_eq!(fleet.uncommitted_events().len(), 2);
    assert_eq!(fleet.state().bicycles()[0].status(), BicycleStatus::Retired);
}

#[test]
fn destination_refusal_leaves_both_fleets_unchanged() {
    let id = BicycleId::new("bike-42").unwrap();
    let mut source = fleet("city-fleet");
    let mut destination = fleet("harbor-fleet");
    source
        .add_bicycle(id.clone(), BicycleCondition::Serviceable)
        .unwrap();
    destination
        .add_bicycle(id.clone(), BicycleCondition::MaintenanceRequired)
        .unwrap();
    let error = BicycleTransfer::transfer(
        &mut source,
        &mut destination,
        &TransferBicycle {
            from_fleet_id: FleetId::new("city-fleet").unwrap(),
            bicycle_id: id,
            to_fleet_id: FleetId::new("harbor-fleet").unwrap(),
        },
    )
    .unwrap_err();
    assert_eq!(
        error.reason,
        BicycleTransferRejectionReason::DestinationAlreadyContainsBicycle
    );
    assert_eq!(source.uncommitted_events().len(), 1);
    assert_eq!(destination.uncommitted_events().len(), 1);
    assert_eq!(source.state().bicycles().len(), 1);
    assert_eq!(destination.state().bicycles().len(), 1);
}
