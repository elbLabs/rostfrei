use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", id = "other", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
