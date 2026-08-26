use rostfrei_domain::{DomainIdentity, Entity, ValueObject};

#[derive(DomainIdentity)]
struct Id(u64);

struct Other;

#[derive(Entity)]
#[domain(id = "bad", label = "Bad", owner = Owner)]
struct Duplicate {
    #[domain(identity, value_object)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "bad", label = "Bad", owner = Owner)]
struct Unsupported {
    #[domain(identity)]
    id: Id,
    #[domain(owns)]
    value: String,
}

#[derive(ValueObject)]
#[domain(id = "bad", label = "Bad", owner = Owner)]
struct EntityInValueObject(#[domain(entity)] Other);

fn main() {}
