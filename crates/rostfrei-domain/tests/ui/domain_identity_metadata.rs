use domain::DomainIdentity;

struct Plain;

#[derive(DomainIdentity)]
struct Missing(u64);

#[derive(DomainIdentity)]
#[domain(owner = Plain, extra = "bad")]
struct Unsupported(u64);

#[derive(DomainIdentity)]
#[domain(owner = Plain, owner = Plain)]
struct Duplicate(u64);

#[derive(DomainIdentity)]
#[domain(owner = Plain)]
struct Wrapper(Option<u64>);

#[derive(DomainIdentity)]
#[domain(owner = Plain)]
struct Reference(&'static str);

fn main() {}
