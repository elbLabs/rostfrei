#![allow(
    clippy::unwrap_used,
    reason = "static test fixture identities must be valid"
)]

use bike_rental::{
    demo::{apply_demo_fixture, demo_stream},
    rental_fleet::{
        self, AddBicycleAction as _, BicycleAdded, BicycleAvailability,
        BicycleAvailabilityQuery as _, BicycleCondition, BicycleId, BicycleNotRented,
        BicycleRented, BicycleRetired, BicycleReturned, BicycleStatus, BicycleTransfer,
        BicycleTransferRejected, BicycleTransferRejectionReason, BicycleTransferredIn,
        BicycleTransferredOut, BicycleUnavailable, FleetId, ImportRentalFleetAction as _,
        ImportRentalFleetInput, ImportedBicycle, RentBicycle, RentBicycleAction as _,
        RentalFleetAggregate, RentalFleetImported, RetireBicycleAction as _,
        ReturnBicycleAction as _, TransferBicycle,
    },
};
use rostfrei::{
    AggregateInstance, CommandOutcome, CommandReceipt, ContentFingerprint, EventVariant,
    ExecutionMetadata, Executor, InMemoryEventStore, OperationId,
};
use uuid::Uuid;

fn fleet_with_bicycles(
    fleet_id: &str,
    bicycles: Vec<ImportedBicycle>,
) -> AggregateInstance<RentalFleetAggregate> {
    let fleet_id = FleetId::new(fleet_id).unwrap();
    AggregateInstance::rehydrate(
        rental_fleet::stream_id(fleet_id.as_str()).unwrap(),
        [RentalFleetImported { fleet_id, bicycles }.into()],
    )
}

fn fleet(
    status: BicycleStatus,
    condition: BicycleCondition,
) -> AggregateInstance<RentalFleetAggregate> {
    fleet_with_bicycles(
        "city-fleet",
        vec![ImportedBicycle {
            bicycle_id: BicycleId::new("bike-42").unwrap(),
            status,
            condition,
        }],
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
        fleet.state().bicycle_availability(&event.bicycle_id),
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

#[test]
fn returns_a_rented_bicycle_and_rejects_a_second_return() {
    let mut fleet = fleet(BicycleStatus::Rented, BicycleCondition::Serviceable);
    let bicycle_id = BicycleId::new("bike-42").unwrap();
    fleet.return_bicycle(bicycle_id.clone()).unwrap();

    let event = EventVariant::<BicycleReturned>::event(&fleet.uncommitted_events()[0]).unwrap();
    assert_eq!(event.bicycle_id, bicycle_id);
    assert_eq!(
        fleet.state().bicycles()[0].status(),
        BicycleStatus::Available
    );
    assert_eq!(
        fleet.state().bicycle_availability(&event.bicycle_id),
        Some(BicycleAvailability::Available)
    );
    assert_eq!(
        fleet.return_bicycle(bicycle_id.clone()),
        Err(BicycleNotRented { bicycle_id })
    );
    assert_eq!(fleet.uncommitted_events().len(), 1);
}

#[test]
fn adds_serviceable_bicycles_with_generated_unique_ids() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    fleet.add_bicycle();

    let event = EventVariant::<BicycleAdded>::event(&fleet.uncommitted_events()[0]).unwrap();
    let first_bicycle_id = event.bicycle_id.clone();
    let expected_id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        b"rostfrei:bike-rental:bicycle:v1:city-fleet:1",
    );
    assert_eq!(event.bicycle_id.as_str(), expected_id.to_string());
    assert_eq!(event.condition, BicycleCondition::Serviceable);
    assert_eq!(fleet.state().bicycles().len(), 2);
    assert_eq!(
        fleet.state().bicycle_availability(&event.bicycle_id),
        Some(BicycleAvailability::Available)
    );
    fleet.add_bicycle();
    let second = EventVariant::<BicycleAdded>::event(&fleet.uncommitted_events()[1]).unwrap();
    assert_ne!(second.bicycle_id, first_bicycle_id);
    assert_eq!(fleet.state().bicycles().len(), 3);
}

#[test]
fn rejects_an_import_with_duplicate_bicycle_identities_without_raising_an_event() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let bicycle_id = BicycleId::new("duplicate-bike").unwrap();
    let input = ImportRentalFleetInput::new(vec![
        ImportedBicycle {
            bicycle_id: bicycle_id.clone(),
            status: BicycleStatus::Available,
            condition: BicycleCondition::Serviceable,
        },
        ImportedBicycle {
            bicycle_id,
            status: BicycleStatus::Rented,
            condition: BicycleCondition::Serviceable,
        },
    ]);

    let error = fleet.import_rental_fleet(input).unwrap_err();

    assert_eq!(error.path, "bicycles");
    assert_eq!(error.reason, "bicycle identities must be unique");
    assert!(fleet.uncommitted_events().is_empty());
    assert_eq!(fleet.state().bicycles().len(), 1);
}

#[test]
fn imports_a_consistent_fleet() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let imported_bicycle = ImportedBicycle {
        bicycle_id: BicycleId::new("bike-84").unwrap(),
        status: BicycleStatus::Rented,
        condition: BicycleCondition::Serviceable,
    };

    fleet
        .import_rental_fleet(ImportRentalFleetInput::new(vec![imported_bicycle.clone()]))
        .unwrap();

    let event = EventVariant::<RentalFleetImported>::event(&fleet.uncommitted_events()[0]).unwrap();
    assert_eq!(event.bicycles, vec![imported_bicycle]);
    assert_eq!(fleet.state().bicycles().len(), 1);
    assert_eq!(fleet.state().bicycles()[0].status(), BicycleStatus::Rented);
}

#[test]
fn retires_available_and_rented_bicycles_through_one_logical_transition() {
    for status in [BicycleStatus::Available, BicycleStatus::Rented] {
        let mut fleet = fleet(status, BicycleCondition::Serviceable);
        let bicycle_id = BicycleId::new("bike-42").unwrap();

        fleet.retire_bicycle(bicycle_id.clone()).unwrap();

        let event = EventVariant::<BicycleRetired>::event(&fleet.uncommitted_events()[0]).unwrap();
        assert_eq!(event.bicycle_id, bicycle_id);
        assert_eq!(fleet.state().bicycles()[0].status(), BicycleStatus::Retired);
    }
}

#[test]
fn rejects_retiring_an_already_retired_bicycle() {
    let mut fleet = fleet(BicycleStatus::Retired, BicycleCondition::Serviceable);
    let bicycle_id = BicycleId::new("bike-42").unwrap();

    let error = fleet.retire_bicycle(bicycle_id.clone()).unwrap_err();

    assert_eq!(error.bicycle_id, bicycle_id);
    assert!(fleet.uncommitted_events().is_empty());
}

#[test]
fn transfers_an_available_bicycle_between_fleets() {
    let bicycle_id = BicycleId::new("bike-42").unwrap();
    let condition = BicycleCondition::MaintenanceRequired;
    let mut source = fleet(BicycleStatus::Available, condition);
    let mut destination = fleet_with_bicycles("harbor-fleet", Vec::new());
    let command = TransferBicycle {
        bicycle_id: bicycle_id.clone(),
        to_fleet_id: FleetId::new("harbor-fleet").unwrap(),
    };

    BicycleTransfer::transfer(&mut source, &mut destination, &command).unwrap();

    let outgoing =
        EventVariant::<BicycleTransferredOut>::event(&source.uncommitted_events()[0]).unwrap();
    assert_eq!(outgoing.bicycle_id, bicycle_id);
    assert_eq!(outgoing.fleet_id.as_str(), "city-fleet");
    assert_eq!(outgoing.to_fleet_id.as_str(), "harbor-fleet");
    assert_eq!(outgoing.condition, condition);
    assert!(source.state().bicycles().is_empty());

    let incoming =
        EventVariant::<BicycleTransferredIn>::event(&destination.uncommitted_events()[0]).unwrap();
    assert_eq!(incoming.bicycle_id, bicycle_id);
    assert_eq!(incoming.fleet_id.as_str(), "harbor-fleet");
    assert_eq!(incoming.from_fleet_id.as_str(), "city-fleet");
    assert_eq!(incoming.condition, condition);
    assert_eq!(destination.state().bicycles().len(), 1);
    assert_eq!(destination.state().bicycles()[0].bicycle_id(), &bicycle_id);
    assert_eq!(
        destination.state().bicycles()[0].status(),
        BicycleStatus::Available
    );
    assert_eq!(destination.state().bicycles()[0].condition(), condition);
}

#[test]
fn rejects_a_transfer_to_the_same_fleet_without_raising_events() {
    let bicycle_id = BicycleId::new("bike-42").unwrap();
    let fleet_id = FleetId::new("city-fleet").unwrap();
    let mut source = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let mut destination = fleet_with_bicycles("city-fleet", Vec::new());
    let command = TransferBicycle {
        bicycle_id: bicycle_id.clone(),
        to_fleet_id: fleet_id.clone(),
    };

    let error = BicycleTransfer::transfer(&mut source, &mut destination, &command).unwrap_err();

    assert_eq!(
        error,
        BicycleTransferRejected {
            bicycle_id,
            from_fleet_id: fleet_id.clone(),
            to_fleet_id: fleet_id,
            reason: BicycleTransferRejectionReason::SameFleet,
        }
    );
    assert!(source.uncommitted_events().is_empty());
    assert!(destination.uncommitted_events().is_empty());
    assert_eq!(source.state().bicycles().len(), 1);
}

#[test]
fn rejects_a_missing_source_bicycle_without_raising_events() {
    let bicycle_id = BicycleId::new("missing-bike").unwrap();
    let mut source = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let mut destination = fleet_with_bicycles("harbor-fleet", Vec::new());
    let command = TransferBicycle {
        bicycle_id: bicycle_id.clone(),
        to_fleet_id: FleetId::new("harbor-fleet").unwrap(),
    };

    let error = BicycleTransfer::transfer(&mut source, &mut destination, &command).unwrap_err();

    assert_eq!(error.bicycle_id, bicycle_id);
    assert_eq!(
        error.reason,
        BicycleTransferRejectionReason::SourceBicycleMissing
    );
    assert!(source.uncommitted_events().is_empty());
    assert!(destination.uncommitted_events().is_empty());
}

#[test]
fn rejects_source_bicycles_that_are_not_available_without_raising_events() {
    for status in [BicycleStatus::Rented, BicycleStatus::Retired] {
        let bicycle_id = BicycleId::new("bike-42").unwrap();
        let mut source = fleet(status, BicycleCondition::Serviceable);
        let mut destination = fleet_with_bicycles("harbor-fleet", Vec::new());
        let command = TransferBicycle {
            bicycle_id,
            to_fleet_id: FleetId::new("harbor-fleet").unwrap(),
        };

        let error = BicycleTransfer::transfer(&mut source, &mut destination, &command).unwrap_err();

        assert_eq!(
            error.reason,
            BicycleTransferRejectionReason::SourceBicycleNotAvailable
        );
        assert!(source.uncommitted_events().is_empty());
        assert!(destination.uncommitted_events().is_empty());
        assert_eq!(source.state().bicycles()[0].status(), status);
    }
}

#[test]
fn validates_the_destination_before_raising_the_outgoing_event() {
    let bicycle_id = BicycleId::new("bike-42").unwrap();
    let mut source = fleet(BicycleStatus::Available, BicycleCondition::Serviceable);
    let mut destination = fleet_with_bicycles(
        "harbor-fleet",
        vec![ImportedBicycle {
            bicycle_id: bicycle_id.clone(),
            status: BicycleStatus::Rented,
            condition: BicycleCondition::MaintenanceRequired,
        }],
    );
    let command = TransferBicycle {
        bicycle_id,
        to_fleet_id: FleetId::new("harbor-fleet").unwrap(),
    };

    let error = BicycleTransfer::transfer(&mut source, &mut destination, &command).unwrap_err();

    assert_eq!(
        error.reason,
        BicycleTransferRejectionReason::DestinationAlreadyContainsBicycle
    );
    assert!(source.uncommitted_events().is_empty());
    assert!(destination.uncommitted_events().is_empty());
    assert_eq!(source.state().bicycles().len(), 1);
    assert_eq!(destination.state().bicycles().len(), 1);
}

#[tokio::test]
async fn rejects_renting_the_same_bicycle_twice_when_commands_are_executed() {
    let store = InMemoryEventStore::new();
    apply_demo_fixture(&store).await.unwrap();
    let bicycle_id = BicycleId::new("bike-42").unwrap();
    let command = RentBicycle {
        bicycle_id: bicycle_id.clone(),
    };

    let outcome = Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(
            ExecutionMetadata::new(
                demo_stream(),
                OperationId::new("rent-bike-42-once").unwrap(),
                ContentFingerprint::digest("rent-bike-42"),
            ),
            &command,
        )
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        CommandOutcome::Accepted(CommandReceipt::Appended(_))
    ));

    let outcome = Executor::new(store.clone())
        .execute::<RentalFleetAggregate, _>(
            ExecutionMetadata::new(
                demo_stream(),
                OperationId::new("rent-bike-42-twice").unwrap(),
                ContentFingerprint::digest("rent-bike-42"),
            ),
            &command,
        )
        .await
        .unwrap();

    match outcome {
        CommandOutcome::Rejected(error) => {
            assert_eq!(error, BicycleUnavailable { bicycle_id });
        }
        outcome @ CommandOutcome::Accepted(_) => {
            panic!("expected domain rejection, got {outcome:?}");
        }
    }
    assert_eq!(store.load(&demo_stream()).await.unwrap().len(), 2);
}
