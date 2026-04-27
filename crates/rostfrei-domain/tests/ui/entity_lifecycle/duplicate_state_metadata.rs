use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    #[state(id = "other", label = "Other")]
    Draft,
}

fn main() {}
