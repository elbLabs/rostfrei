use rostfrei_domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow", owner = Todo, initial = Draft)]
enum Lifecycle<T> {
    #[domain(id = "draft", label = "Draft")]
    Draft(T),
}

fn main() {}
