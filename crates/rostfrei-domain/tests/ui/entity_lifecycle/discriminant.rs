use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft = 1,
}

fn main() {}
