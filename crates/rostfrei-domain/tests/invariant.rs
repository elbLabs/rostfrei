use domain::{InvariantDescriptor, InvariantId, InvariantViolation, domain_invariant};

#[domain_invariant(id = "unique-bicycles", label = "Unique bicycles")]
trait UniqueBicycles {
    fn validate(&self) -> Option<InvariantViolation>;
}

struct RentalFleet {
    has_duplicate: bool,
}

impl UniqueBicycles for RentalFleet {
    fn validate(&self) -> Option<InvariantViolation> {
        self.has_duplicate
            .then(|| InvariantViolation::new("bicycles", "bicycle identities must be unique"))
    }
}

struct EmptyFleet;

impl UniqueBicycles for EmptyFleet {
    fn validate(&self) -> Option<InvariantViolation> {
        None
    }
}

#[test]
fn annotated_trait_keeps_ordinary_behavior_and_global_metadata() {
    let violation = RentalFleet {
        has_duplicate: true,
    }
    .validate();

    assert_eq!(
        violation,
        Some(InvariantViolation::new(
            "bicycles",
            "bicycle identities must be unique"
        ))
    );
    assert_eq!(
        <RentalFleet as UniqueBicycles>::DESCRIPTOR,
        InvariantDescriptor {
            id: InvariantId("unique-bicycles"),
            label: "Unique bicycles",
        }
    );
    assert_eq!(
        <RentalFleet as UniqueBicycles>::DESCRIPTOR,
        <EmptyFleet as UniqueBicycles>::DESCRIPTOR
    );
}
rostfrei_domain_macros::__install_test_macro_support!();
