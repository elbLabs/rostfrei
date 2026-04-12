use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = Actions::__DOMAIN_ACTION_REFERENCE_ACTIVATE, to = Draft)]
    Draft,
}

fn main() {}
