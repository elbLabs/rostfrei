#![allow(dead_code)]

use async_trait::async_trait;
use domain::{
    Aggregate as DomainAggregate, AggregateDefinition, AggregateEvents, AggregateType,
    BoundedContext, BoundedContextType, Command, DomainEvent, DomainIdentity, Entity,
    JsonCommandPayload,
};
use rostfrei_core::{
    AggregateInstance, CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult,
};
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
#[domain(context = Catalog, id = "open-catalog", label = "Open catalog")]
struct OpenCatalog;

#[derive(Command)]
#[domain(context = Catalog, id = "describe-catalog", label = "Describe catalog")]
struct DescribeCatalog;

#[derive(Debug, Command, Eq, PartialEq)]
#[domain(context = Catalog, id = "rename-catalog", label = "Rename catalog")]
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

struct OpenCatalogHandler;

#[async_trait]
impl CommandHandler<OpenCatalog> for OpenCatalogHandler {
    type Rejection = std::convert::Infallible;

    async fn handle(
        &self,
        command: &OpenCatalog,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut aggregate = execution.load::<CatalogAggregate>("catalog").await?;
        match aggregate.aggregate_mut().open_catalog(command) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => match rejection {},
        }
    }
}

#[test]
fn registers_handler_linked_runtime_metadata() {
    let mut registry = DomainRegistry::new();
    registry
        .register_command::<OpenCatalog, OpenCatalogHandler>()
        .unwrap();

    let descriptor = registry.command("catalog", "open-catalog", 1).unwrap();

    assert_eq!(descriptor.bounded_context, "catalog");
    assert_eq!(descriptor.modeled_command(), &OpenCatalog::DESCRIPTOR);
    assert_eq!(
        <OpenCatalog as CommandDefinition<OpenCatalogHandler>>::descriptor().bounded_context,
        Catalog::DESCRIPTOR.id.0
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
