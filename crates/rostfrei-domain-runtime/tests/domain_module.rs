#![allow(dead_code)]

use domain::{
    Aggregate as DomainAggregate, AggregateType, BoundedContext, Command, CommandType, DomainEvent,
    DomainIdentity, Entity, JsonCommandPayload,
};
use rostfrei_core::{Aggregate as RuntimeAggregate, AggregateInstance, CommandHandler};
use rostfrei_domain_runtime::{Apply, Initialize, domain_module};
use rostfrei_registry::{CommandDefinition, DomainRegistry, QueryDefinition};
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

#[derive(Command)]
#[domain(
    id = "open-catalog",
    label = "Open catalog",
    owner = CatalogAggregate,
    runtime
)]
struct OpenCatalog;

#[derive(Command)]
#[domain(
    id = "describe-catalog",
    label = "Describe catalog",
    owner = CatalogAggregate
)]
struct DescribeCatalog;

#[derive(Debug, Command, Eq, PartialEq)]
#[domain(
    id = "rename-catalog",
    label = "Rename catalog",
    owner = CatalogAggregate,
    json
)]
struct RenameCatalog {
    r#type: String,
}

struct CatalogSummary;

impl QueryDefinition for CatalogSummary {
    type Response = String;

    const BOUNDED_CONTEXT: &'static str = "catalog";
    const QUERY_NAME: &'static str = "catalog-summary";
    const SCHEMA_VERSION: u32 = 1;
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

domain_module! {
    struct CatalogModule {
        commands: [OpenCatalog],
        queries: [CatalogSummary],
    }
}

#[test]
fn registers_runtime_metadata_from_the_command() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<CatalogModule>().unwrap();

    let descriptor = registry
        .command("catalog/catalog", "open-catalog", 1)
        .unwrap();

    assert_eq!(descriptor.aggregate_type, "catalog/catalog");
    assert_eq!(descriptor.modeled_command(), Some(&OpenCatalog::DESCRIPTOR));
    assert_eq!(
        <OpenCatalog as CommandDefinition>::Aggregate::AGGREGATE_TYPE,
        "catalog"
    );
    assert_eq!(
        <OpenCatalog as CommandDefinition>::Aggregate::aggregate_type().as_ref(),
        "catalog/catalog"
    );
    assert_eq!(CatalogAggregate::DESCRIPTOR.id.local, "catalog");
    assert_eq!(
        registry.query("catalog", "catalog-summary", 1),
        Some(&CatalogSummary::descriptor())
    );
}

#[test]
fn commands_can_register_without_a_runtime_module() {
    let mut registry = DomainRegistry::new();

    registry.register_command::<OpenCatalog>().unwrap();

    assert_eq!(registry.modules().count(), 0);
    assert_eq!(
        registry
            .command("catalog/catalog", "open-catalog", 1)
            .unwrap()
            .modeled_command(),
        Some(&OpenCatalog::DESCRIPTOR)
    );
}

#[test]
fn command_metadata_does_not_require_an_executable_handler() {
    assert_eq!(DescribeCatalog::DESCRIPTOR.id.local, "describe-catalog");
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
