use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Missing)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
