use rostfrei_domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "duplicate", label = "Duplicate", owner = Owner)]
struct Duplicate(#[domain(scalar = Provider, scalar = Provider)] Custom);

#[derive(ValueObject)]
#[domain(id = "conflicting", label = "Conflicting", owner = Owner)]
struct Conflicting(#[domain(scalar = Provider, value_object)] Custom);

#[derive(DomainIdentity)]
#[domain(owner = Entity, scalar = Provider, scalar = Provider)]
struct DuplicateIdentity(Custom);

fn main() {}
