use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
