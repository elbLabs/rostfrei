use domain::Entity;

struct Id;

#[derive(Entity)]
#[domain(id = "named", label = "Named", owner = Owner)]
struct Tuple(Id);

#[derive(Entity)]
#[domain(id = "generic", label = "Generic", owner = Owner)]
struct Generic<T> {
    #[domain(identity)]
    id: T,
}

fn main() {}
