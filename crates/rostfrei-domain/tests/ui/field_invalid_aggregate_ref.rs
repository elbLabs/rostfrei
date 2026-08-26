use rostfrei_domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
#[domain(owner = MissingValue)]
struct Id(u64);

#[derive(DomainIdentity)]
#[domain(owner = GenericTarget)]
struct GenericId(u64);

#[derive(Entity)]
#[domain(id = "missing", label = "Missing", owner = Owner)]
struct MissingValue {
    #[domain(identity)]
    id: GenericId,
    #[domain(aggregate_ref)]
    target: Id,
}

#[derive(Entity)]
#[domain(id = "generic", label = "Generic", owner = Owner)]
struct GenericTarget {
    #[domain(identity)]
    id: Id,
    #[domain(aggregate_ref = Vec<Target>)]
    target: Id,
}

fn main() {}
