use domain::DomainService;

#[derive(DomainService)]
#[domain(id = "named", label = "Named")]
struct Named { value: u64 }

#[derive(DomainService)]
#[domain(id = "tuple", label = "Tuple")]
struct Tuple(u64);

#[derive(DomainService)]
#[domain(id = "generic", label = "Generic")]
struct Generic<T>(T);

#[derive(DomainService)]
#[domain(id = "enumerated", label = "Enumerated")]
enum Enumerated {}

fn main() {}
