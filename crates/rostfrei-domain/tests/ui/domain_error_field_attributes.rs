use domain::DomainError;

#[derive(DomainError)]
#[domain(id = "denied", label = "Denied", code = "DENIED", message = "Denied.")]
struct Denied {
    #[domain(field)]
    value: String,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
