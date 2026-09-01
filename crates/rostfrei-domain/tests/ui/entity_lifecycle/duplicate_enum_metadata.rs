use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
#[domain(id = "other", label = "Other")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
}

fn main() {}
