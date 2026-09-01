use domain::EntityLifecycle;

#[derive(EntityLifecycle)]
#[domain(id = "workflow", label = "Workflow")]
enum Workflow {
    #[state(id = "draft", label = "Draft")]
    Draft,
    #[state(id = "draft", label = "Also draft")]
    AlsoDraft,
}

fn main() {}
