use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[domain(id = "other", label = "Other")]
    Draft,
}

fn main() {}
