use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = Actions::activate, to = Draft)]
    Draft,
}

fn main() {}
