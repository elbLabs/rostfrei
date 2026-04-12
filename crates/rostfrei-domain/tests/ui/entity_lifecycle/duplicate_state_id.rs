use rostfrei_domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "same", label = "Draft")]
    Draft,
    #[domain(id = "same", label = "Active")]
    Active,
}

fn main() {}
