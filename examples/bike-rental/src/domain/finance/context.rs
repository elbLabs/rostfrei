use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "finance", label = "Finance")]
pub struct Finance;
