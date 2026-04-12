use rostfrei_domain::DomainService;

#[derive(DomainService)]
#[domain(label = "Missing id", context = Context)]
struct MissingId;

#[derive(DomainService)]
#[domain(id = "missing-label", context = Context)]
struct MissingLabel;

#[derive(DomainService)]
#[domain(id = "missing-context", label = "Missing context")]
struct MissingContext;

#[derive(DomainService)]
#[domain(id = "Invalid", label = "Invalid", context = Context)]
struct InvalidId;

#[derive(DomainService)]
#[domain(id = "blank-label", label = " ", context = Context)]
struct BlankLabel;

#[derive(DomainService)]
#[domain(id = "unsupported", label = "Unsupported", context = Context, owner = Context)]
struct Unsupported;

fn main() {}
