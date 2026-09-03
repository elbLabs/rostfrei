use domain::{DecisionDescriptor, DecisionId, domain_decision};

#[domain_decision(id = "eligible", label = "Eligible")]
trait Eligible {
    fn eligible(&self) -> bool;
}

struct Fleet;

impl Eligible for Fleet {
    fn eligible(&self) -> bool {
        true
    }
}

fn main() {
    let descriptor: DecisionDescriptor = <Fleet as Eligible>::DESCRIPTOR;
    assert_eq!(descriptor.id, DecisionId("eligible"));
    assert!(Fleet.eligible());
}
rostfrei_domain_macros::__install_test_macro_support!();
