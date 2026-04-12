use rostfrei_domain::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "inbox", id = "other", label = "Inbox")]
struct DuplicateKey;

#[derive(BoundedContext)]
#[domain(id = "support", label = "Support", schema = 1)]
struct UnsupportedKey;

#[derive(BoundedContext)]
#[domain(id = "billing", label = "Billing")]
#[domain(id = "other", label = "Other")]
struct DuplicateAttribute;

fn main() {}
