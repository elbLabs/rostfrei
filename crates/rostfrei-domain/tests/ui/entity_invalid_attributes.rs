use domain::Entity;

struct Id(u64);

#[derive(Entity)]
struct Missing {
    id: Id,
}

#[derive(Entity)]
#[domain(id = "Bad--Id", label = "Bad")]
struct MalformedId {
    id: Id,
}

impl domain::EntityDefinition for MalformedId {
    type Owner = Owner;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Entity)]
#[domain(id = "missing-owner", label = "Missing Owner")]
struct MissingOwner {
    id: Id,
}

fn main() {}
