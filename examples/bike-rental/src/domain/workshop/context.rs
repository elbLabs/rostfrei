use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "workshop", label = "Workshop")]
pub struct Workshop;
