use domain::Entity;

struct Id;
trait Actions {}

#[derive(Entity)]
#[domain(id = "item", label = "Item", owner = Owner, actions = [Actions, Actions])]
struct Item {
    #[domain(identity)]
    id: Id,
}

fn main() {}
