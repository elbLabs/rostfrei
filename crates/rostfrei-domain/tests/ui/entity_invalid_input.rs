use domain::Entity;

struct Id;

#[derive(Entity)]
#[domain(id = "named", label = "Named")]
struct Tuple(Id);

#[derive(Entity)]
#[domain(id = "generic", label = "Generic")]
struct Generic<T> {
    id: T,
}

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
