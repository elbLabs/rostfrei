use rostfrei_domain::ValueObject;

struct PlainOwner;

#[derive(ValueObject)]
#[domain(id = "email-address", label = "Email address", owner = PlainOwner)]
struct EmailAddress(String);

fn main() {}
