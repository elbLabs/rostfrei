use domain::DomainEvent;

struct Owner;

#[derive(DomainEvent)]
struct MissingDomain;

#[derive(DomainEvent)]
#[domain(label = "Missing id")]
struct MissingId;

#[derive(DomainEvent)]
#[domain(id = "missing-owner", label = "Missing owner")]
struct MissingOwner;

#[derive(DomainEvent)]
#[domain(id = "duplicate", id = "other", label = "Duplicate")]
struct Duplicate;

#[derive(DomainEvent)]
#[domain(id = "unsupported", label = "Unsupported", schema = 1)]
struct Unsupported;

#[derive(DomainEvent)]
#[domain(id = "Bad--Id", label = "Malformed")]
struct MalformedId;

#[derive(DomainEvent)]
#[domain(id = "blank-label", label = " ")]
struct BlankLabel;

#[derive(DomainEvent)]
#[domain(id = "unsupported-owner", label = "Unsupported owner", owner = Owner)]
struct UnsupportedOwner;

fn main() {}
