use domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "generic-field", label = "Generic field", owner = Owner)]
struct GenericField(#[domain(scalar = Provider<u64>)] Custom);

#[derive(DomainIdentity)]
#[domain(owner = Entity, scalar = Provider<u64>)]
struct GenericIdentity(Custom);

fn main() {}
