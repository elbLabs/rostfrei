use rostfrei_domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(Entity)]
struct Missing {
    #[domain(identity)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "Bad--Id", label = "Bad", owner = Owner)]
struct MalformedId {
    #[domain(identity)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "missing-owner", label = "Missing Owner")]
struct MissingOwner {
    #[domain(identity)]
    id: Id,
}

fn main() {}
