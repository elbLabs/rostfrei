use domain::{QueryDescriptor, QueryId, domain_query};

#[domain_query(id = "available", label = "Available")]
trait Available {
    fn available(&self, id: u64) -> bool;
}

struct Catalog;

impl Available for Catalog {
    fn available(&self, id: u64) -> bool {
        id > 0
    }
}

fn main() {
    let descriptor: QueryDescriptor = <Catalog as Available>::DESCRIPTOR;
    assert_eq!(descriptor.id, QueryId("available"));
    assert!(Catalog.available(1));
}
