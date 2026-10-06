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

// This consumer has only one (renamed) framework dependency. Each feature is
// checked independently so workspace feature unification cannot hide a missing edge.
fn assert_facade_contracts() {
    fn event_store<S: rf::EventStore>() {}
    fn message_adapter<A: rf::CommandMessageAdapter>() {}
    let _ = rf::core::JsonEventCodec;
    let _: Option<rf::messaging::ConsumerConfig<rf::CommandAddress>> = None;
    let _ = rf::registry::DomainRegistry::default();
    #[cfg(feature = "nats")]
    {
        event_store::<rf::nats::NatsEventStore>();
        message_adapter::<rf::nats::NatsMessagingAdapter>();
    }
    #[cfg(feature = "http")]
    let _ = rf::http::HttpApiConfig::default();
    #[cfg(feature = "tracer")]
    {
        let _ = rf::tracer::ApplicationBindings::new();
        let _ = rf::tracer::http::HttpConfig::new("test-token");
    }
    #[cfg(all(feature = "nats", feature = "tracer"))]
    let _ = rf::tracer::NatsApplication::builder;
    #[cfg(any(feature = "fixtures", feature = "tracer", feature = "testing"))]
    let _: Option<rf::fixtures::Fixture> = None;
    #[cfg(feature = "testing")]
    let _ = rf::testing::given::<CatalogAggregate, Vec<CatalogEvents>>;
}
