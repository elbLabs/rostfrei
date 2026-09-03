use rostfrei_macros::QueryDefinition;

#[derive(QueryDefinition)]
#[rostfrei(
    context = "catalog",
    name = "find-product",
    version = 0,
    response = ()
)]
struct FindProduct;

fn main() {}
