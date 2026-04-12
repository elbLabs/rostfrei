use rostfrei_domain::ValueObject;

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address", owner = Owner)]
struct EmailAddress(#[domain(identity)] String);

fn main() {}
