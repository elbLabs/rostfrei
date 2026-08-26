use domain::ValueObject;

struct Owner;

#[derive(ValueObject)]
struct MissingDomain;

#[derive(ValueObject)]
#[domain(label = "Missing id", owner = Owner)]
struct MissingId;

#[derive(ValueObject)]
#[domain(id = "duplicate", id = "other", label = "Duplicate", owner = Owner)]
struct Duplicate;

#[derive(ValueObject)]
#[domain(id = "unsupported", label = "Unsupported", owner = Owner, schema = 1)]
struct Unsupported;

#[derive(ValueObject)]
#[domain(id = "Bad--Id", label = "Malformed", owner = Owner)]
struct MalformedId;

#[derive(ValueObject)]
#[domain(id = "blank-label", label = "  ", owner = Owner)]
struct BlankLabel;

#[derive(ValueObject)]
#[domain(id = "malformed-owner", label = "Malformed owner", owner = "Owner")]
struct MalformedOwner;

fn main() {}
