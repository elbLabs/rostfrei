use rostfrei_macros::{Module, QueryDefinition};

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

#[derive(Module)]
#[rostfrei(name = "catalog", queries(FindProduct))]
struct Catalog;

fn main() {
    let _ = <FindProduct as zs_registry::QueryDefinition>::descriptor();
    let _ = <Catalog as zs_registry::DomainModule>::descriptor();
}
