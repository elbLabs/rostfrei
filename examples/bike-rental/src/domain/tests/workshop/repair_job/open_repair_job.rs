use rostfrei::{AggregateInstance, StreamAggregateId, StreamAggregateType, StreamId};

use crate::domain::{
    rental_fleet::{BicycleId, FleetId},
    repair_job::{OpenRepairJobAction as _, RepairDetails, RepairJobAggregate},
};

#[test]
fn reopening_a_job_preserves_its_original_details() {
    let stream = StreamId::new(
        StreamAggregateType::new("bike-rental/repair-job").expect("valid aggregate type"),
        StreamAggregateId::new("brake-service-42").expect("valid job identity"),
    );
    let details = RepairDetails {
        fleet_id: FleetId::new("city-fleet").expect("valid fleet identity"),
        bicycle_id: BicycleId::new("bike-42").expect("valid bicycle identity"),
        damage_report: "Rear brake does not engage".to_owned(),
    };
    let mut job = AggregateInstance::<RepairJobAggregate>::rehydrate(stream.clone(), vec![]);
    job.open_repair_job(details.clone())
        .expect("first job opens");
    let mut replay = AggregateInstance::<RepairJobAggregate>::rehydrate(stream, job.into_parts().2);
    replay
        .open_repair_job(details.clone())
        .expect("same job is accepted without another event");
    let conflicting = RepairDetails {
        damage_report: "Flat front tyre".to_owned(),
        ..details.clone()
    };
    assert!(replay.open_repair_job(conflicting).is_err());
    assert!(replay.uncommitted_events().is_empty());
    assert_eq!(replay.state().details(), Some(&details));
}
