use domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
#[domain(owner = MissingValue)]
struct Id(u64);

#[derive(DomainIdentity)]
#[domain(owner = GenericTarget)]
struct GenericId(u64);

#[derive(Entity)]
#[domain(id = "missing", label = "Missing")]
struct MissingValue {
    #[domain(identity)]
    id: GenericId,
    #[domain(aggregate_ref)]
    target: Id,
}

impl domain::EntityDefinition for MissingValue {
    type Owner = Owner;
    type Identity = GenericId;
}

#[derive(Entity)]
#[domain(id = "generic", label = "Generic")]
struct GenericTarget {
    #[domain(identity)]
    id: Id,
    #[domain(aggregate_ref = Vec<Target>)]
    target: Id,
}

impl domain::EntityDefinition for GenericTarget {
    type Owner = Owner;
    type Identity = Id;
}

fn main() {}
