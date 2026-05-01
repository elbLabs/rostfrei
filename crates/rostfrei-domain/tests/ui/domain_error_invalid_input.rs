use domain::DomainError;

#[derive(DomainError)]
#[domain(id = "choice", label = "Choice", code = "CHOICE", message = "Choice.")]
enum Choice {
    First,
}

#[derive(DomainError)]
#[domain(id = "storage", label = "Storage", code = "STORAGE", message = "Storage.")]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(DomainError)]
#[domain(id = "generic", label = "Generic", code = "GENERIC", message = "Generic.")]
struct Generic<T>(T);

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
