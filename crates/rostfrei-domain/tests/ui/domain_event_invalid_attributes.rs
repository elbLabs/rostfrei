use domain::DomainEvent;

struct Owner;

#[derive(DomainEvent)]
struct MissingDomain;

#[derive(DomainEvent)]
#[domain(label = "Missing id", owner = Owner)]
struct MissingId;

#[derive(DomainEvent)]
#[domain(id = "missing-owner", label = "Missing owner")]
struct MissingOwner;

#[derive(DomainEvent)]
#[domain(id = "duplicate", id = "other", label = "Duplicate", owner = Owner)]
struct Duplicate;

#[derive(DomainEvent)]
#[domain(id = "unsupported", label = "Unsupported", owner = Owner, schema = 1)]
struct Unsupported;

#[derive(DomainEvent)]
#[domain(id = "Bad--Id", label = "Malformed", owner = Owner)]
struct MalformedId;

#[derive(DomainEvent)]
#[domain(id = "blank-label", label = " ", owner = Owner)]
struct BlankLabel;

#[derive(DomainEvent)]
#[domain(id = "malformed-owner", label = "Malformed owner", owner = "Owner")]
struct MalformedOwner;

fn main() {}
