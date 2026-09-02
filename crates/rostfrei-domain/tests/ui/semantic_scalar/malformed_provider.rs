use domain::{DomainIdentity, ValueObject};

struct Custom;

#[derive(ValueObject)]
#[domain(id = "malformed-field", label = "Malformed field", owner = Owner)]
struct MalformedField(#[domain(scalar = "Provider")] Custom);

#[derive(DomainIdentity)]
struct MalformedIdentity(Custom);

fn main() {}
