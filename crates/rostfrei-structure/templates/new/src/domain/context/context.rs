use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "{{context_id}}", label = "{{context_label}}")]
pub struct {{context_type}};
