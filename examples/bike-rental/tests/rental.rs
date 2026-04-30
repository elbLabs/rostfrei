#![allow(
    clippy::unwrap_used,
    reason = "static test fixture identities must be valid"
)]

use bike_rental::{
    demo::{demo_stream, seed_demo},
    rental_fleet::{
        self, AddBicycleAction as _, BicycleAdded, BicycleAvailability, BicycleCondition,
        BicycleId, BicycleNotRented, BicycleRented, BicycleReturned, BicycleStatus,
        BicycleUnavailable, FleetId, ImportedBicycle, RentBicycle, RentBicycleAction as _,
        RentalFleetAggregate, RentalFleetImported, ReturnBicycleAction as _,
    },
};
use rostfrei::{
    AggregateInstance, CommandOutcome, CommandReceipt, ContentFingerprint, EventVariant,
    ExecutionMetadata, Executor, InMemoryEventStore, OperationId,
};
use uuid::Uuid;

fn fleet(
    status: BicycleStatus,
    condition: BicycleCondition,
) -> AggregateInstance<RentalFleetAggregate> {
    let fleet_id = FleetId::new("city-fleet").unwrap();
    AggregateInstance::rehydrate(
        rental_fleet::stream_id(fleet_id.as_str()).unwrap(),
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
        RentalFleetAggregate::bicycle_availability(fleet.state(), &event.bicycle_id),
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
        RentalFleetAggregate::bicycle_availability(fleet.state(), &event.bicycle_id),
        Some(BicycleAvailability::Available)
    );
    fleet.add_bicycle();
    let second = EventVariant::<BicycleAdded>::event(&fleet.uncommitted_events()[1]).unwrap();
    assert_ne!(second.bicycle_id, first_bicycle_id);
    assert_eq!(fleet.state().bicycles().len(), 3);
}

#[tokio::test]
async fn rejects_renting_the_same_bicycle_twice_when_commands_are_executed() {
    let store = InMemoryEventStore::new();
    seed_demo(&store).await.unwrap();
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
