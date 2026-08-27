#![allow(dead_code)]

use domain::{
    Aggregate as DomainAggregate, AggregateType, BoundedContext, DomainCommand, DomainCommandType,
    DomainEvent, DomainIdentity, Entity,
};
use rostfrei_core::{Aggregate as RuntimeAggregate, AggregateInstance};
use rostfrei_domain_runtime::{domain_command_handler, domain_module, Apply, Initialize};
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

trait CatalogRuntimeActions {
    fn open_catalog(&mut self, command: &OpenCatalog) -> Result<(), std::convert::Infallible>;
}

impl CatalogRuntimeActions for AggregateInstance<CatalogAggregate> {
    fn open_catalog(&mut self, _command: &OpenCatalog) -> Result<(), std::convert::Infallible> {
        self.raise(CatalogOpened);
        Ok(())
    }
}

domain_command_handler!(OpenCatalog => open_catalog);

domain_module! {
    struct CatalogModule {
        commands: [OpenCatalog],
    }
}

#[test]
fn registers_runtime_metadata_from_the_domain_command() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<CatalogModule>().unwrap();

    let descriptor = registry
        .command("catalog/catalog", "open-catalog", 1)
        .unwrap();

    assert_eq!(descriptor.aggregate_type, "catalog/catalog");
    assert_eq!(descriptor.domain_command(), Some(&OpenCatalog::DESCRIPTOR));
    assert_eq!(
        <OpenCatalog as CommandDefinition>::Aggregate::AGGREGATE_TYPE,
        "catalog"
    );
    assert_eq!(
        <OpenCatalog as CommandDefinition>::Aggregate::aggregate_type().as_ref(),
        "catalog/catalog"
    );
    assert_eq!(CatalogAggregate::DESCRIPTOR.id.local, "catalog");
}
