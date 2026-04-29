use domain::ValueObject;

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address")]
struct EmailAddress(#[domain(identity)] String);

fn main() {}
