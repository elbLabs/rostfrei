use domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(DomainIdentity)]
struct GenericId(u64);

#[derive(Entity)]
#[domain(id = "missing", label = "Missing")]
struct MissingValue {
    id: GenericId,
    #[domain(aggregate_ref)]
    target: Id,
}

impl domain::EntityDefinition for MissingValue {
    type Owner = Owner;
    type Identity = GenericId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Entity)]
#[domain(id = "generic", label = "Generic")]
struct GenericTarget {
    id: Id,
    #[domain(aggregate_ref = Vec<Target>)]
    target: Id,
}

impl domain::EntityDefinition for GenericTarget {
    type Owner = Owner;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

fn main() {}
