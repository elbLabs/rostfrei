use domain::ValueObject;

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address")]
struct EmailAddress(#[domain(entity)] String);

fn main() {}
