use rostfrei_domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "malformed-field", label = "Malformed field", owner = Owner)]
struct MalformedField(#[domain(scalar = "Provider")] Custom);

#[derive(DomainIdentity)]
#[domain(owner = Entity, scalar = "Provider")]
struct MalformedIdentity(Custom);

fn main() {}
