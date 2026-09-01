use domain::{DomainIdentity, Entity, ValueObject};

#[derive(DomainIdentity)]
struct Id(u64);

struct Other;

#[derive(Entity)]
#[domain(id = "bad", label = "Bad", owner = Owner)]
struct Duplicate {
    #[domain(identity, value_object)]
    id: Id,
}
}

#[derive(Entity)]
#[domain(id = "bad", label = "Bad")]
struct Unsupported {
    #[domain(identity)]
    id: Id,
    #[domain(owns)]
    value: String,
}

impl domain::EntityDefinition for Unsupported {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(ValueObject)]
#[domain(id = "bad", label = "Bad", owner = Owner)]
struct EntityInValueObject(#[domain(entity)] Other);

fn main() {}
