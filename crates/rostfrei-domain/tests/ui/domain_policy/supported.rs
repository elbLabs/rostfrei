use domain::{PolicyDescriptor, PolicyId, domain_policy};

#[domain_policy(id = "eligible", label = "Eligible")]
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
    let descriptor: PolicyDescriptor = <Fleet as Eligible>::DESCRIPTOR;
    assert_eq!(descriptor.id, PolicyId("eligible"));
    assert!(Fleet.eligible());
}
rostfrei_domain_macros::__install_test_macro_support!();
