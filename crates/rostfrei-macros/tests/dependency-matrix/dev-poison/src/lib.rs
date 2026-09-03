#![allow(dead_code)]

rostfrei::install_macro_support!();

#[derive(rostfrei::BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(rostfrei::DomainIdentity)]
struct CatalogId(u64);

#[derive(rostfrei::Entity)]
#[domain(id = "catalog-root", label = "Catalog root")]
struct CatalogRoot {
    id: CatalogId,
    opened: bool,
}

impl rostfrei::EntityDefinition for CatalogRoot {
    type Owner = CatalogAggregate;
    type Identity = CatalogId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize, rostfrei::DomainEvent)]
#[domain(id = "catalog-opened", label = "Catalog opened")]
struct CatalogOpened;

#[derive(rostfrei::AggregateEvents)]
enum CatalogEvents {
    Opened(CatalogOpened),
}

#[derive(rostfrei::Aggregate)]
#[domain(id = "catalog", label = "Catalog")]
struct CatalogAggregate;

impl rostfrei::AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = CatalogEvents;
}

impl rostfrei::Initialize<CatalogAggregate> for CatalogRoot {
    fn initialize(_: &rostfrei::StreamId) -> Self {
        Self {
            id: CatalogId(1),
            opened: false,
        }
    }
}

impl rostfrei::Apply<CatalogOpened> for CatalogRoot {
    fn apply(&mut self, _: &CatalogOpened) {
        self.opened = true;
    }
}

#[derive(rostfrei::QueryDefinition)]
#[rostfrei(
    context = "catalog",
    name = "catalog-status",
    version = 1,
    response = bool
)]
struct CatalogStatus;

fn assert_generated_contracts() {
    fn aggregate<A: rostfrei::AggregateRuntime>() {}
    aggregate::<CatalogAggregate>();
    let _ = <CatalogStatus as rostfrei::QueryDefinition>::descriptor();
}
