#![allow(dead_code)]

use domain::{
    Aggregate as DomainAggregate, AggregateDefinition, AggregateEvents, AggregateType,
    BoundedContext, Command, DomainEvent, DomainIdentity, Entity, JsonCommandPayload,
};
use rostfrei_core::{Aggregate as RuntimeAggregate, AggregateInstance, CommandHandler};
use rostfrei_domain_runtime::{Apply, Initialize};
use rostfrei_registry::{CommandDefinition, DomainRegistry};
use serde::{Deserialize, Serialize};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
struct CatalogId(u64);

#[derive(Entity)]
#[domain(id = "catalog-root", label = "Catalog")]
struct CatalogRoot {
    id: CatalogId,
    opened: bool,
}

impl domain::EntityDefinition for CatalogRoot {
    type Owner = CatalogAggregate;
    type Identity = CatalogId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[test]
fn generic_runtime_code_can_read_an_entity_identity() {
    fn identity_of<E: domain::EntityDefinition>(entity: &E) -> &E::Identity {
        entity.identity()
    }

    let root = CatalogRoot {
        id: CatalogId(7),
        opened: false,
    };
    assert_eq!(identity_of(&root).0, 7);
}

#[derive(DomainAggregate)]
#[domain(id = "catalog", label = "Catalog")]
struct CatalogAggregate;

impl AggregateDefinition for CatalogAggregate {
    type Context = Catalog;
    type Root = CatalogRoot;
    type Event = CatalogEvents;
}

#[derive(AggregateEvents)]
enum CatalogEvents {
    Opened(CatalogOpened),
}

#[derive(Command)]
#[domain(id = "open-catalog", label = "Open catalog")]
struct OpenCatalog;

#[derive(Command)]
#[domain(id = "describe-catalog", label = "Describe catalog")]
struct DescribeCatalog;

#[derive(Debug, Command, Eq, PartialEq)]
#[domain(id = "rename-catalog", label = "Rename catalog")]
struct RenameCatalog {
    r#type: String,
}

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

impl CommandHandler<OpenCatalog> for CatalogAggregate {
    type Rejection = std::convert::Infallible;

    fn handle(
        command: &OpenCatalog,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.open_catalog(command)
    }
}

#[test]
fn registers_handler_linked_runtime_metadata() {
    let mut registry = DomainRegistry::new();
    registry
        .register_command::<CatalogAggregate, OpenCatalog>()
        .unwrap();

    let descriptor = registry
        .command("catalog/catalog", "open-catalog", 1)
        .unwrap();

    assert_eq!(descriptor.aggregate_type, "catalog/catalog");
    assert_eq!(descriptor.modeled_command(), &OpenCatalog::DESCRIPTOR);
    assert_eq!(
        <OpenCatalog as CommandDefinition<CatalogAggregate>>::descriptor().aggregate_type,
        CatalogAggregate::aggregate_type()
    );
    assert_eq!(CatalogAggregate::DESCRIPTOR.id.local, "catalog");
}

#[test]
fn command_metadata_does_not_require_an_executable_handler() {
    assert_eq!(DescribeCatalog::DESCRIPTOR.local_id, "describe-catalog");
}

fn assert_raw_identifier_command(command: &RenameCatalog) {
    assert_eq!(command.r#type, "wholesale");
}

#[test]
fn json_command_preserves_raw_identifier_wire_names() -> Result<(), String> {
    let command = RenameCatalog::decode_json(&domain::__private::serde_json::json!({
        "type": "wholesale",
    }))?;

    assert_raw_identifier_command(&command);
    Ok(())
}
#[doc(hidden)]
pub mod __rostfrei_macro_support {
    pub use domain::*;
    pub use rostfrei_domain_runtime::*;

    macro_rules! __runtime {
        ($($tokens:tt)*) => {
            $($tokens)*
        };
    }
    pub(crate) use __runtime;

    pub mod __private {
        pub use domain::__private::*;
        pub use rostfrei_domain_runtime::__private::{assert_unique_event_ids, core};
    }
}
