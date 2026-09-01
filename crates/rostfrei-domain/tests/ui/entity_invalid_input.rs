use domain::Entity;

struct Id;

#[derive(Entity)]
#[domain(id = "named", label = "Named")]
struct Tuple(Id);

#[derive(Entity)]
#[domain(id = "generic", label = "Generic")]
struct Generic<T> {
    #[domain(identity)]
    id: T,
}

fn main() {}
