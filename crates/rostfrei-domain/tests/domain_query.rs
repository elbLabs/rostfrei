use domain::{QueryDescriptor, QueryId, domain_query};

#[domain_query(id = "bicycle-availability", label = "Bicycle availability")]
trait BicycleAvailabilityQuery {
    fn bicycle_available(&self, bicycle_id: u64) -> bool;
}

struct RentalFleet {
    available: Vec<u64>,
}

impl BicycleAvailabilityQuery for RentalFleet {
    fn bicycle_available(&self, bicycle_id: u64) -> bool {
        self.available.contains(&bicycle_id)
    }
}

struct EmptyFleet;

impl BicycleAvailabilityQuery for EmptyFleet {
    fn bicycle_available(&self, _bicycle_id: u64) -> bool {
        false
    }
}

#[test]
fn annotated_trait_keeps_ordinary_behavior_and_global_metadata() {
    let fleet = RentalFleet {
        available: vec![1, 3],
    };

    assert!(fleet.bicycle_available(3));
    assert!(!fleet.bicycle_available(2));
    assert_eq!(
        <RentalFleet as BicycleAvailabilityQuery>::DESCRIPTOR,
        QueryDescriptor {
            id: QueryId("bicycle-availability"),
            label: "Bicycle availability",
        }
    );
    assert_eq!(
        <RentalFleet as BicycleAvailabilityQuery>::DESCRIPTOR,
        <EmptyFleet as BicycleAvailabilityQuery>::DESCRIPTOR
    );
}
