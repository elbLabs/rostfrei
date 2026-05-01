use domain::DomainEvent;

struct Owner;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created", owner = Owner)]
struct Created;

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
