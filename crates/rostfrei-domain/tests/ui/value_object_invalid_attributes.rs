use domain::ValueObject;

struct Owner;

#[derive(ValueObject)]
struct MissingDomain;

#[derive(ValueObject)]
#[domain(label = "Missing id", owner = Owner)]
struct MissingId;

#[derive(ValueObject)]
#[domain(id = "duplicate", label = "Duplicate")]
struct Duplicate;

#[derive(ValueObject)]
#[domain(id = "unsupported", label = "Unsupported")]
struct Unsupported;

#[derive(ValueObject)]
#[domain(id = "Bad--Id", label = "Malformed")]
struct MalformedId;

#[derive(ValueObject)]
#[domain(id = "blank-label", label = "  ")]
struct BlankLabel;

#[derive(ValueObject)]
#[domain(id = "malformed-owner", label = "Malformed owner")]
struct MalformedOwner;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
