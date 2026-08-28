use std::io;

use bike_rental::rental::{
    BicycleAvailability, BicycleCondition, BicycleId, BicycleRented, BicycleStatus,
    BicycleUnavailable, FleetId, ImportedBicycle, RentalFleetActions, RentalFleetAggregate,
    RentalFleetImported,
};
use rostfrei::{AggregateInstance, EventVariant, StreamAggregateId, StreamAggregateType, StreamId};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn fleet(
    status: BicycleStatus,
    condition: BicycleCondition,
) -> TestResult<AggregateInstance<RentalFleetAggregate>> {
    let fleet_id = FleetId::new("city-fleet")
        .ok_or_else(|| io::Error::other("fixture fleet ID should be non-empty"))?;
    Ok(AggregateInstance::rehydrate(
        StreamId::new(
            StreamAggregateType::new("bike-rental/rental-fleet")?,
            StreamAggregateId::new(fleet_id.as_str())?,
        ),
        [RentalFleetImported {
            fleet_id,
            bicycles: vec![ImportedBicycle {
                bicycle_id: BicycleId::new("bike-42")
                    .ok_or_else(|| io::Error::other("fixture bicycle ID should be non-empty"))?,
                status,
                condition,
            }],
        }
        .into()],
    ))
}

#[test]
fn rents_an_available_serviceable_bicycle() {
    let mut fleet = fleet(BicycleStatus::Available, BicycleCondition::Serviceable).unwrap();
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
    )
    .unwrap();
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
fn model_projects_each_executable_fleet_action_once() {
    let model = bike_rental::domain_model();
    let actions = model["actions"].as_array().unwrap();
    let fleet_actions = actions
        .iter()
        .filter(|action| {
            action["id"]["owner"]["kind"] == "aggregate"
                && action["id"]["owner"]["id"]["context"] == "bike-rental"
                && action["id"]["owner"]["id"]["local"] == "rental-fleet"
        })
        .collect::<Vec<_>>();

    assert_eq!(
        fleet_actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["import-rental-fleet", "rent-bicycle"]
    );
    assert!(
        fleet_actions
            .iter()
            .all(|action| action["output"].is_null())
    );
    assert_eq!(
        fleet_actions
            .iter()
            .map(|action| action["raises"][0]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["rental-fleet-imported", "bicycle-rented"]
    );
}
