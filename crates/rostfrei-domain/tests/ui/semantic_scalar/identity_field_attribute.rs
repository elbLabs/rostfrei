use rostfrei_domain::DomainIdentity;

struct Custom;

#[derive(DomainIdentity)]
#[domain(owner = Entity, scalar = Provider)]
struct Invalid(#[domain(scalar = Provider)] Custom);

fn main() {}
