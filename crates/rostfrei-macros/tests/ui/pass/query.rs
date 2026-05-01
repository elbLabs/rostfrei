use rostfrei_macros::QueryDefinition;

#[derive(QueryDefinition)]
#[rostfrei(
    context = "catalog",
    name = "find-product",
    version = 1,
    response = Option<String>
)]
struct FindProduct {
    product_id: String,
}

fn main() {
    let _ = <FindProduct as zs_registry::QueryDefinition>::descriptor();
}
