use rostfrei_domain::DomainService;

#[derive(DomainService)]
#[domain(id = "named", label = "Named", context = Context)]
struct Named { value: u64 }

#[derive(DomainService)]
#[domain(id = "tuple", label = "Tuple", context = Context)]
struct Tuple(u64);

#[derive(DomainService)]
#[domain(id = "generic", label = "Generic", context = Context)]
struct Generic<T>(T);

#[derive(DomainService)]
#[domain(id = "enumerated", label = "Enumerated", context = Context)]
enum Enumerated {}

fn main() {}
