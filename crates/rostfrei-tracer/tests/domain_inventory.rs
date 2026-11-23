//! Broker-free contract: declarations, registrations, and configured storage are distinct.
#![allow(clippy::unwrap_used)]

use domain::{
    __private::DomainModelBuilder, AggregateDescriptor, AggregateId, BoundedContextDescriptor,
    BoundedContextId, DomainIdentityId, EntityDescriptor, EntityId,
};
use rostfrei_core::{Aggregate, ReadModel, StreamId};
use rostfrei_messaging_core::ApplicationName;
use rostfrei_registry::{DomainRegistry, QueryDefinition};
use rostfrei_tracer::domain_inventory::{DomainInventory, ReadModelDeclaration};
use serde::{Deserialize, Serialize};
use serde_json::json;

struct Order;

impl Aggregate for Order {
    type State = ();
    type Event = ();
    const BOUNDED_CONTEXT: &'static str = "orders";
    const AGGREGATE_TYPE: &'static str = "orders/order";
    fn initial(_: &StreamId) {}
    fn apply((): &mut (), (): &()) {}
}

struct BillingView;

impl QueryDefinition for BillingView {
    type Response = ();
    const BOUNDED_CONTEXT: &'static str = "billing";
    const QUERY_NAME: &'static str = "billing-view";
    const SCHEMA_VERSION: u32 = 2;
}

#[test]
fn compiled_declarations_survive_without_runtime_registration() {
    let context = BoundedContextId("orders");
    let aggregate = AggregateId {
        context,
        local: "order",
    };
    let entity = EntityId {
        aggregate,
        local: "line",
    };
    let mut model = DomainModelBuilder::new();
    model
        .add_bounded_context(BoundedContextDescriptor {
            id: context,
            label: "Orders",
        })
        .unwrap();
    model
        .add_bounded_context(BoundedContextDescriptor {
            id: BoundedContextId("returns"),
            label: "Returns",
        })
        .unwrap();
    model
        .add_aggregate(AggregateDescriptor {
            id: aggregate,
            label: "Order",
            root: entity,
        })
        .unwrap();
    model
        .add_entity(EntityDescriptor {
            id: entity,
            label: "Line",
            identity: DomainIdentityId { owner: entity },
            fields: &[],
        })
        .unwrap();
    let model = model.finish().unwrap();
    let mut registry = DomainRegistry::new();
    registry.register_aggregate::<Order>().unwrap();
    registry.register_query::<BillingView>().unwrap();

    let inventory = DomainInventory::from_model(Some(&model), &registry).unwrap();
    let document = serde_json::to_value(inventory).unwrap();
    assert_eq!(
        document.get("boundedContexts"),
        Some(&json!([
            {"id":"billing"}, {"id":"orders", "label":"Orders"}, {"id":"returns", "label":"Returns"}
        ]))
    );
    assert_eq!(
        document.pointer("/aggregates/0/aggregateType"),
        Some(&json!("orders/order"))
    );
    assert_eq!(
        document.pointer("/aggregates/0/root"),
        document.pointer("/entities/0/id")
    );
    assert_eq!(document.get("entities"), model.get("entities"));
    assert_eq!(
        document.get("registeredQueries"),
        Some(&json!([
            {"context":"billing", "name":"billing-view", "schemaVersion":2}
        ]))
    );

    let declarations_only =
        DomainInventory::from_model(Some(&model), &DomainRegistry::new()).unwrap();
    assert!(
        declarations_only
            .aggregates
            .first()
            .unwrap()
            .aggregate_type
            .is_none()
    );

    let registrations_only = DomainInventory::from_model(None, &registry).unwrap();
    assert!(!registrations_only.compiled_model_available);
    assert!(registrations_only.entities.is_empty());
    let registered = registrations_only.aggregates.first().unwrap();
    assert_eq!(registered.aggregate_type.as_deref(), Some("orders/order"));
    assert!(registered.root.is_none());
}

#[derive(Default, Serialize, Deserialize)]
struct LineSummary;

impl ReadModel for LineSummary {
    const NAME: &'static str = "line-summary";
    const SCHEMA_VERSION: u32 = 3;
}

#[test]
fn a_read_model_declaration_does_not_imply_nats_storage() {
    let context = ApplicationName::new("sample")
        .unwrap()
        .bounded_context("orders")
        .unwrap();
    let declaration = ReadModelDeclaration::for_model::<LineSummary>(context.name()).unwrap();
    assert_eq!(
        serde_json::to_value(declaration).unwrap(),
        json!({
            "context":"orders", "name":"line-summary", "schemaVersion":3
        })
    );
}

#[cfg(feature = "nats-host")]
#[test]
fn explicit_nats_policy_uses_its_own_resource_identity_scope_and_budget() {
    use rostfrei_nats::NatsReadModelConfig;

    let context = ApplicationName::new("sample")
        .unwrap()
        .test_bounded_context("orders")
        .unwrap();
    let config = NatsReadModelConfig::for_model::<LineSummary>(&context)
        .unwrap()
        .with_storage_limits(9_007_199_254_740_993, 4096)
        .unwrap();
    let declaration = ReadModelDeclaration::from_nats_config::<LineSummary>(&config).unwrap();
    let document = serde_json::to_value(declaration).unwrap();
    assert_eq!(
        document.get("storage"),
        Some(&json!({
            "kind":"natsKv", "application":"sample", "scope":"test",
            "bucketName":config.bucket_name(), "streamName":config.stream_name(),
            "maxBytes":"9007199254740993", "maxValueBytes":4096,
            "storage":"file", "replicas":1, "history":1, "ttlNanos":"0"
        }))
    );
    let other = NatsReadModelConfig::new(&context, "other").unwrap();
    assert!(ReadModelDeclaration::from_nats_config::<LineSummary>(&other).is_none());
}
