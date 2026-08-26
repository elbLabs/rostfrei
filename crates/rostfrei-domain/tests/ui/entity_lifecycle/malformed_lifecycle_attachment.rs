use domain::Entity;

#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = Owner,
    lifecycle = Lifecycle<u8>
)]
struct Todo {
    id: u8,
}

fn main() {}
