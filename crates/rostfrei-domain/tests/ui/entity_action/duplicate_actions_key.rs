use rostfrei_domain::Entity;

struct Id;
trait Actions {}

#[derive(Entity)]
#[domain(
    id = "item",
    label = "Item",
    owner = Owner,
    actions = [Actions],
    actions = [Actions]
)]
struct Item {
    #[domain(identity)]
    id: Id,
}

fn main() {}
