#![allow(dead_code)]

use rostfrei_core::{Aggregate as RuntimeAggregate, CommandHandler, DecisionContext};
use rostfrei_domain::{
    Aggregate as DomainAggregate, AggregateType, BoundedContext, DomainCommand, DomainCommandType,
    DomainIdentity, Entity,
};
use rostfrei_domain_runtime::{domain_module, AggregateRuntime};
use rostfrei_registry::{CommandDefinition, DomainRegistry};

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
}

#[derive(DomainAggregate)]
#[domain(id = "catalog", label = "Catalog", context = Catalog, root = CatalogRoot)]
struct CatalogAggregate;

#[derive(DomainCommand)]
#[domain(id = "open-catalog", label = "Open catalog", owner = CatalogAggregate)]
struct OpenCatalog;

#[derive(Default)]
struct CatalogState {
    opened: bool,
}

enum CatalogEvent {
    Opened,
}

impl RuntimeAggregate for CatalogState {
    type Event = CatalogEvent;

    const AGGREGATE_TYPE: &'static str = "catalog";

    fn initial() -> Self {
        Self::default()
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            CatalogEvent::Opened => self.opened = true,
        }
    }
}

impl AggregateRuntime for CatalogAggregate {
    type Runtime = CatalogState;
}

impl CommandHandler<OpenCatalog> for CatalogState {
    type Rejection = ();

    fn handle(
        _command: &OpenCatalog,
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        context.record(CatalogEvent::Opened);
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
