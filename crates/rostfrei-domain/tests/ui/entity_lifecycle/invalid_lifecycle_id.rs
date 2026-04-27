use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "Invalid", label = "Workflow")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
