use domain::DomainService;

#[derive(DomainService)]
#[domain(label = "Missing id")]
struct MissingId;

#[derive(DomainService)]
#[domain(id = "missing-label")]
struct MissingLabel;

#[derive(DomainService)]
#[domain(id = "Invalid", label = "Invalid")]
struct InvalidId;

#[derive(DomainService)]
#[domain(id = "blank-label", label = " ")]
struct BlankLabel;

#[derive(DomainService)]
#[domain(id = "unsupported", label = "Unsupported", context = Context)]
struct Unsupported;

fn main() {}
