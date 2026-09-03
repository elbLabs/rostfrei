#![allow(dead_code)]

rf::install_macro_support!();

#[derive(rf::BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(rf::DomainIdentity)]
struct CatalogId(u64);

#[derive(rf::Entity)]
#[domain(id = "catalog-root", label = "Catalog root")]
struct CatalogRoot {
    id: CatalogId,
    opened: bool,
}

impl rf::EntityDefinition for CatalogRoot {
    type Owner = CatalogAggregate;
    type Identity = CatalogId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize, rf::DomainEvent)]
#[domain(id = "catalog-opened", label = "Catalog opened")]
struct CatalogOpened;

#[derive(rf::AggregateEvents)]
enum CatalogEvents {
    Opened(CatalogOpened),
}

#[derive(rf::Aggregate)]
#[domain(id = "catalog", label = "Catalog")]
struct CatalogAggregate;

impl rf::AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = CatalogEvents;
}

impl rf::Initialize<CatalogAggregate> for CatalogRoot {
    fn initialize(_: &rf::StreamId) -> Self {
        Self {
            id: CatalogId(1),
            opened: false,
        }
    }
}

impl rf::Apply<CatalogOpened> for CatalogRoot {
    fn apply(&mut self, _: &CatalogOpened) {
        self.opened = true;
    }
}

#[derive(rf::QueryDefinition)]
#[rostfrei(
    context = "catalog",
    name = "catalog-status",
    version = 1,
    response = bool
)]
struct CatalogStatus;

fn assert_generated_contracts() {
    fn aggregate<A: rf::AggregateRuntime>() {}
    aggregate::<CatalogAggregate>();
    let _ = <CatalogStatus as rf::QueryDefinition>::descriptor();
}
