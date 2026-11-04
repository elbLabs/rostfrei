#![allow(dead_code)]
use domain::{
    Aggregate, AggregateEventSet, AggregateType, BoundedContext, DomainEventDescriptor,
    DomainEventId, DomainIdentity, DomainIdentityId, DomainModelError, Entity, EntityDescriptor,
    EntityId, EntityType, domain_model,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;
#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Another Rust type")]
struct DuplicateCatalog;
#[derive(DomainIdentity)]
struct RootId;
#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    id: RootId,
}
impl domain::EntityDefinition for Root {
    type Owner = Eventful;
    type Identity = RootId;
    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}
#[derive(Aggregate)]
#[domain(id = "eventful", label = "Eventful")]
struct Eventful;
impl domain::AggregateDefinition for Eventful {
    type Context = Catalog;
    type Root = Root;
    type Event = DuplicateEvents;
}
enum DuplicateEvents {}
const EVENT: DomainEventDescriptor = DomainEventDescriptor {
    id: DomainEventId {
        aggregate: Eventful::DESCRIPTOR.id,
        local: "changed",
    },
    label: "Changed",
    schema_version: 1,
    fields: &[],
};
impl AggregateEventSet<Eventful> for DuplicateEvents {
    const DOMAIN_EVENTS: &'static [DomainEventDescriptor] = &[EVENT, EVENT];
}

#[test]
fn model_identity_is_semantic_rather_than_rust_type_identity() {
    assert!(matches!(
        domain_model! { contexts: [Catalog, DuplicateCatalog], aggregates: [], entities: [], value_objects: [], services: [], errors: [] },
        Err(DomainModelError::DuplicateBoundedContextId { .. }),
    ));
}

#[test]
fn duplicate_events_leave_neither_the_aggregate_nor_partial_events() {
    let mut builder = domain::__private::DomainModelBuilder::new();
    assert_eq!(
        builder.add_aggregate_type::<Eventful>().unwrap_err(),
        DomainModelError::DuplicateDomainEventId {
            id: Box::new(EVENT.id)
        }
    );
    let model = builder.finish().unwrap();
    assert!(model["aggregates"].as_array().unwrap().is_empty());
    assert!(model["domainEvents"].as_array().unwrap().is_empty());
}

#[test]
fn identity_collision_does_not_leave_a_partially_inserted_entity() {
    let original = Root::DESCRIPTOR;
    let borrowed = EntityDescriptor {
        id: EntityId {
            aggregate: original.id.aggregate,
            local: "other",
        },
        label: "Other",
        identity: DomainIdentityId { owner: original.id },
        fields: &[],
    };
    let mut builder = domain::__private::DomainModelBuilder::new();
    builder.add_entity(borrowed).unwrap();
    assert!(matches!(
        builder.add_entity(original),
        Err(DomainModelError::DuplicateDomainIdentityId { .. })
    ));
    let model = builder.finish().unwrap();
    assert_eq!(model["entities"].as_array().unwrap().len(), 1);
    assert_eq!(model["domainIdentities"].as_array().unwrap().len(), 1);
}

rostfrei_domain_macros::__install_test_macro_support!();
