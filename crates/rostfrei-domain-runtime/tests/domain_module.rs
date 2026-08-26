#![allow(dead_code)]

use domain::{
    Aggregate as DomainAggregate, AggregateType, BoundedContext, DomainCommand, DomainCommandType,
    DomainEvent, DomainIdentity, Entity,
};
use rostfrei_core::{Aggregate as RuntimeAggregate, CommandHandler, DecisionContext};
use rostfrei_domain_runtime::{domain_module, Apply, Initialize};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::{Deserialize, Serialize};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = CatalogRoot)]
struct CatalogId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog", owner = CatalogAggregate)]
struct CatalogRoot {
    #[domain(identity)]
    id: CatalogId,
    opened: bool,
}

#[derive(DomainAggregate)]
#[domain(
    id = "catalog",
    label = "Catalog",
    context = Catalog,
    root = CatalogRoot,
    events = [CatalogOpened]
)]
struct CatalogAggregate;

#[derive(DomainCommand)]
#[domain(id = "open-catalog", label = "Open catalog", owner = CatalogAggregate)]
struct OpenCatalog;

#[derive(Deserialize, DomainEvent, Serialize)]
#[domain(id = "catalog-opened", label = "Catalog opened")]
struct CatalogOpened;

impl Initialize<CatalogAggregate> for CatalogRoot {
    fn initialize(_stream_id: &rostfrei_core::StreamId) -> Self {
        Self {
            id: CatalogId(0),
            opened: false,
        }
    }
}

impl Apply<CatalogOpened> for CatalogRoot {
    fn apply(&mut self, _event: &CatalogOpened) {
        self.opened = true;
    }
}

impl CommandHandler<OpenCatalog> for CatalogAggregate {
    type Rejection = ();

    fn handle(
        _command: &OpenCatalog,
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        context.record(CatalogOpened);
        Ok(())
    }
}

domain_module! {
    struct CatalogModule {
        name: "catalog",
        commands: [
            OpenCatalog => {
                name: "catalog.open-catalog",
                version: 1,
            },
        ],
    }
}

#[test]
fn registers_runtime_metadata_from_the_domain_command() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<CatalogModule>().unwrap();

    let descriptor = registry.command("catalog.open-catalog", 1).unwrap();

    assert_eq!(descriptor.aggregate_type, "catalog");
    assert_eq!(descriptor.domain_command(), Some(&OpenCatalog::DESCRIPTOR));
    assert_eq!(
        <OpenCatalog as CommandDefinition>::Aggregate::AGGREGATE_TYPE,
        "catalog"
    );
    assert_eq!(CatalogAggregate::DESCRIPTOR.id.local, "catalog");
}
