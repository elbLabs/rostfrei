use rostfrei::BoundedContext;

#[derive(BoundedContext)]
#[domain(id = "rental", label = "Rental")]
pub struct Rental;
