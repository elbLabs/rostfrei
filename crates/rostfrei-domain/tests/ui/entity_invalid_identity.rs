use domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
struct Id(u64);

#[derive(Entity)]
#[domain(id = "missing", label = "Missing", owner = Owner)]
struct Missing {
    id: Id,
}

#[derive(Entity)]
#[domain(id = "multiple", label = "Multiple", owner = Owner)]
struct Multiple {
    #[domain(identity)]
    first: Id,
    #[domain(identity)]
    second: Id,
}

#[derive(Entity)]
#[domain(id = "unsupported", label = "Unsupported", owner = Owner)]
struct Unsupported {
    #[domain(primary)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "syntax", label = "Syntax", owner = Owner)]
struct Syntax {
    #[domain(identity = true)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "duplicate", label = "Duplicate", owner = Owner)]
struct Duplicate {
    #[domain(identity)]
    #[domain(identity)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "wrapped", label = "Wrapped", owner = Owner)]
struct Wrapped {
    #[domain(identity)]
    id: Option<Id>,
}

fn main() {}
