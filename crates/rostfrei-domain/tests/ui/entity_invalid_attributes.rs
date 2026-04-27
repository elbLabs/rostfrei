use domain::Entity;

struct Id(u64);

#[derive(Entity)]
struct Missing {
    #[domain(identity)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "Bad--Id", label = "Bad")]
struct MalformedId {
    #[domain(identity)]
    id: Id,
}

impl domain::EntityDefinition for MalformedId {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "missing-owner", label = "Missing Owner")]
struct MissingOwner {
    #[domain(identity)]
    id: Id,
}

fn main() {}
