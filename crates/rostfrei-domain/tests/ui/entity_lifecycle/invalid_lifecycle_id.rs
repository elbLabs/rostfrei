use rostfrei_domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "Bad Id", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
