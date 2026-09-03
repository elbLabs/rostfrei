use domain::DomainEvent;

#[derive(DomainEvent)]
#[domain(id = "created", label = "Created")]
struct Created {
    #[domain(field)]
    value: String,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
