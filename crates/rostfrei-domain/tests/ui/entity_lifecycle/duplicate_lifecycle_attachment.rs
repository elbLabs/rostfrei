use rostfrei_domain::Entity;

#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = Owner,
    lifecycle = First,
    lifecycle = Second
)]
struct Todo {
    id: u8,
}

fn main() {}
