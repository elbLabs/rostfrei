use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(
    // Keep the context ID stable when changing its display label.
    id = "{{context_id}}",
    label = "{{context_label}}"
)]
pub struct {{context_type}};
