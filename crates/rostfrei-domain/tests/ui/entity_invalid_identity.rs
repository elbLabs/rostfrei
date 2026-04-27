use domain::Entity;

struct Id(u64);

#[derive(Entity)]
#[domain(id = "missing", label = "Missing")]
struct Missing {
    id: Id,
}

#[derive(Entity)]
#[domain(id = "multiple", label = "Multiple")]
struct Multiple {
    #[domain(identity)]
    first: Id,
    #[domain(identity)]
    second: Id,
}

#[derive(Entity)]
#[domain(id = "unsupported", label = "Unsupported")]
struct Unsupported {
    #[domain(primary)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "syntax", label = "Syntax")]
struct Syntax {
    #[domain(identity = true)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "duplicate", label = "Duplicate")]
struct Duplicate {
    #[domain(identity)]
    #[domain(identity)]
    id: Id,
}

#[derive(Entity)]
#[domain(id = "wrapped", label = "Wrapped")]
struct Wrapped {
    #[domain(identity)]
    id: Option<Id>,
}

fn main() {}
