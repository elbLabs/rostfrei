#![allow(dead_code)]

use domain::{
    AggregateDefinition, AggregateDescriptor, AggregateEventSet, AggregateId, AggregateType,
    BoundedContextDescriptor, BoundedContextId, BoundedContextType, DomainError, DomainErrorId,
    DomainEventDescriptor, DomainEventId, DomainIdentity, DomainIdentityId, DomainModelError,
    DomainServiceDefinition, DomainServiceDescriptor, DomainServiceId, DomainServiceType,
    EntityDefinition, EntityDescriptor, EntityId, EntityType, NoDomainEvents, ValueObject,
    ValueObjectDescriptor, ValueObjectId, domain_model,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("catalog");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "primary",
};
const ROOT_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "root",
};

struct Catalog;
impl BoundedContextType for Catalog {
    const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
        id: CONTEXT_ID,
        label: "Catalog",
    };
}
struct DuplicateCatalog;
impl BoundedContextType for DuplicateCatalog {
    const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
        id: CONTEXT_ID,
        label: "Duplicate catalog",
    };
}

struct RootIdentity(u64);
impl DomainIdentity for RootIdentity {}
struct Root {
    id: RootIdentity,
}
impl EntityType for Root {
    const LOCAL_ID: &'static str = "root";
    const DESCRIPTOR: EntityDescriptor = entity_descriptor("Root");
}
impl EntityDefinition for Root {
    type Owner = PrimaryAggregate;
    type Identity = RootIdentity;
    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

struct PrimaryAggregate;
impl AggregateType for PrimaryAggregate {
    const DESCRIPTOR: AggregateDescriptor = aggregate_descriptor("Primary aggregate");
}
impl AggregateDefinition for PrimaryAggregate {
    type Context = Catalog;
    type Root = Root;
    type Event = NoDomainEvents;
}

struct DuplicateAggregateIdentity(u64);
impl DomainIdentity for DuplicateAggregateIdentity {}
struct DuplicateAggregateRoot {
    id: DuplicateAggregateIdentity,
}
impl EntityType for DuplicateAggregateRoot {
    const LOCAL_ID: &'static str = "root";
    const DESCRIPTOR: EntityDescriptor = entity_descriptor("Duplicate aggregate root");
}
impl EntityDefinition for DuplicateAggregateRoot {
    type Owner = DuplicateAggregate;
    type Identity = DuplicateAggregateIdentity;
    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}
struct DuplicateAggregate;
impl AggregateType for DuplicateAggregate {
    const DESCRIPTOR: AggregateDescriptor = aggregate_descriptor("Duplicate aggregate");
}
impl AggregateDefinition for DuplicateAggregate {
    type Context = Catalog;
    type Root = DuplicateAggregateRoot;
    type Event = NoDomainEvents;
}

struct DuplicateEntityIdentity(u64);
impl DomainIdentity for DuplicateEntityIdentity {}
struct DuplicateEntity {
    id: DuplicateEntityIdentity,
}
impl EntityType for DuplicateEntity {
    const LOCAL_ID: &'static str = "root";
    const DESCRIPTOR: EntityDescriptor = entity_descriptor("Duplicate entity");
}
impl EntityDefinition for DuplicateEntity {
    type Owner = PrimaryAggregate;
    type Identity = DuplicateEntityIdentity;
    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

const fn entity_descriptor(label: &'static str) -> EntityDescriptor {
    EntityDescriptor {
        id: ROOT_ID,
        label,
        identity: DomainIdentityId { owner: ROOT_ID },
        fields: &[],
    }
}
const fn aggregate_descriptor(label: &'static str) -> AggregateDescriptor {
    AggregateDescriptor {
        id: AGGREGATE_ID,
        label,
        root: ROOT_ID,
    }
}

struct Quantity(u64);
impl ValueObject for Quantity {
    const DESCRIPTOR: ValueObjectDescriptor = ValueObjectDescriptor {
        id: ValueObjectId("quantity"),
        label: "Quantity",
    };
}
struct DuplicateQuantity(u64);
impl ValueObject for DuplicateQuantity {
    const DESCRIPTOR: ValueObjectDescriptor = ValueObjectDescriptor {
        id: ValueObjectId("quantity"),
        label: "Duplicate quantity",
    };
}

struct Pricing;
impl DomainServiceType for Pricing {
    const DESCRIPTOR: DomainServiceDescriptor = service_descriptor("Pricing");
}
impl DomainServiceDefinition for Pricing {
    type Context = Catalog;
}
struct DuplicatePricing;
impl DomainServiceType for DuplicatePricing {
    const DESCRIPTOR: DomainServiceDescriptor = service_descriptor("Duplicate pricing");
}
impl DomainServiceDefinition for DuplicatePricing {
    type Context = Catalog;
}
const fn service_descriptor(label: &'static str) -> DomainServiceDescriptor {
    DomainServiceDescriptor {
        id: DomainServiceId {
            context: CONTEXT_ID,
            local: "pricing",
        },
        label,
    }
}

const EVENTFUL_AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "eventful",
};
const EVENTFUL_ROOT_ID: EntityId = EntityId {
    aggregate: EVENTFUL_AGGREGATE_ID,
    local: "root",
};
const DUPLICATE_EVENT_ID: DomainEventId = DomainEventId {
    aggregate: EVENTFUL_AGGREGATE_ID,
    local: "changed",
};
struct EventfulIdentity(u64);
impl DomainIdentity for EventfulIdentity {}
struct EventfulRoot {
    id: EventfulIdentity,
}
impl EntityType for EventfulRoot {
    const LOCAL_ID: &'static str = "root";
    const DESCRIPTOR: EntityDescriptor = EntityDescriptor {
        id: EVENTFUL_ROOT_ID,
        label: "Eventful root",
        identity: DomainIdentityId {
            owner: EVENTFUL_ROOT_ID,
        },
        fields: &[],
    };
}
impl EntityDefinition for EventfulRoot {
    type Owner = EventfulAggregate;
    type Identity = EventfulIdentity;
    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}
struct EventfulAggregate;
impl AggregateType for EventfulAggregate {
    const DESCRIPTOR: AggregateDescriptor = AggregateDescriptor {
        id: EVENTFUL_AGGREGATE_ID,
        label: "Eventful aggregate",
        root: EVENTFUL_ROOT_ID,
    };
}
impl AggregateDefinition for EventfulAggregate {
    type Context = Catalog;
    type Root = EventfulRoot;
    type Event = DuplicateEvents;
}
enum DuplicateEvents {}
impl AggregateEventSet<EventfulAggregate> for DuplicateEvents {
    const DOMAIN_EVENTS: &'static [DomainEventDescriptor] = &[
        event_descriptor("First event"),
        event_descriptor("Second event"),
    ];
}
const fn event_descriptor(label: &'static str) -> DomainEventDescriptor {
    DomainEventDescriptor {
        id: DUPLICATE_EVENT_ID,
        label,
        schema_version: 1,
        fields: &[],
    }
}

struct CatalogError;
impl DomainError for CatalogError {
    const LOCAL_ID: &'static str = "catalog-error";
    const LABEL: &'static str = "Catalog error";
    const CODE: &'static str = "CATALOG_ERROR";
    const MESSAGE: &'static str = "Catalog operation failed.";
    const FIELDS: &'static [domain::FieldDescriptor] = &[];
}
struct DuplicateCatalogError;
impl DomainError for DuplicateCatalogError {
    const LOCAL_ID: &'static str = "catalog-error";
    const LABEL: &'static str = "Duplicate catalog error";
    const CODE: &'static str = "DUPLICATE_CATALOG_ERROR";
    const MESSAGE: &'static str = "Duplicate catalog operation failed.";
    const FIELDS: &'static [domain::FieldDescriptor] = &[];
}

fn empty_model_with(
    configure: impl FnOnce(&mut domain::__private::DomainModelBuilder) -> Result<(), DomainModelError>,
) -> Result<serde_json::Value, DomainModelError> {
    let mut builder = domain::__private::DomainModelBuilder::new();
    configure(&mut builder)?;
    builder.finish()
}

#[test]
fn rejects_same_and_distinct_context_types_with_the_same_id() {
    let expected = DomainModelError::DuplicateBoundedContextId {
        id: Box::new(CONTEXT_ID),
    };
    for actual in [
        domain_model! { contexts: [Catalog, Catalog], aggregates: [], entities: [], value_objects: [], services: [], errors: [] },
        domain_model! { contexts: [Catalog, DuplicateCatalog], aggregates: [], entities: [], value_objects: [], services: [], errors: [] },
    ] {
        assert_eq!(actual.expect_err("duplicate context must fail"), expected);
    }
}

#[test]
fn rejects_same_and_distinct_aggregate_types_with_the_same_id() {
    let expected = DomainModelError::DuplicateAggregateId {
        id: Box::new(AGGREGATE_ID),
    };
    for actual in [
        domain_model! { contexts: [], aggregates: [PrimaryAggregate, PrimaryAggregate], entities: [], value_objects: [], services: [], errors: [] },
        domain_model! { contexts: [], aggregates: [PrimaryAggregate, DuplicateAggregate], entities: [], value_objects: [], services: [], errors: [] },
    ] {
        assert_eq!(actual.expect_err("duplicate aggregate must fail"), expected);
    }
}

#[test]
fn rejects_same_and_distinct_entity_types_with_the_same_id() {
    let expected = DomainModelError::DuplicateEntityId {
        id: Box::new(ROOT_ID),
    };
    for actual in [
        domain_model! { contexts: [], aggregates: [], entities: [Root, Root], value_objects: [], services: [], errors: [] },
        domain_model! { contexts: [], aggregates: [], entities: [Root, DuplicateEntity], value_objects: [], services: [], errors: [] },
    ] {
        assert_eq!(actual.expect_err("duplicate entity must fail"), expected);
    }
}

#[test]
fn rejects_same_and_distinct_value_object_types_with_the_same_id() {
    let expected = DomainModelError::DuplicateValueObjectId {
        id: Box::new(ValueObjectId("quantity")),
    };
    for actual in [
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [Quantity, Quantity], services: [], errors: [] },
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [Quantity, DuplicateQuantity], services: [], errors: [] },
    ] {
        assert_eq!(
            actual.expect_err("duplicate value object must fail"),
            expected
        );
    }
}

#[test]
fn rejects_same_and_distinct_service_types_with_the_same_id() {
    let expected = DomainModelError::DuplicateDomainServiceId {
        id: Box::new(Pricing::DESCRIPTOR.id),
    };
    for actual in [
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [], services: [Pricing, Pricing], errors: [] },
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [], services: [Pricing, DuplicatePricing], errors: [] },
    ] {
        assert_eq!(actual.expect_err("duplicate service must fail"), expected);
    }
}

#[test]
fn preserves_same_and_distinct_domain_error_duplicate_validation() {
    let expected = DomainModelError::DuplicateDomainErrorId {
        id: Box::new(DomainErrorId("catalog-error")),
    };
    for actual in [
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [], services: [], errors: [CatalogError, CatalogError] },
        domain_model! { contexts: [], aggregates: [], entities: [], value_objects: [], services: [], errors: [CatalogError, DuplicateCatalogError] },
    ] {
        assert_eq!(actual.expect_err("duplicate error must fail"), expected);
    }
}

#[test]
fn aggregate_and_event_batch_is_atomic() {
    let mut builder = domain::__private::DomainModelBuilder::new();
    let error = builder
        .add_aggregate_type::<EventfulAggregate>()
        .expect_err("duplicate event batch must fail");
    assert_eq!(
        error,
        DomainModelError::DuplicateDomainEventId {
            id: Box::new(DUPLICATE_EVENT_ID),
        }
    );
    let model = builder.finish().expect("failed batch must be atomic");
    assert!(model["aggregates"].as_array().unwrap().is_empty());
    assert!(model["domainEvents"].as_array().unwrap().is_empty());
}

#[test]
fn aggregate_collision_precedes_event_validation() {
    let model = empty_model_with(|builder| {
        builder.add_aggregate_type::<PrimaryAggregate>()?;
        assert_eq!(
            builder
                .add_aggregate_type::<PrimaryAggregate>()
                .expect_err("duplicate aggregate must fail"),
            DomainModelError::DuplicateAggregateId {
                id: Box::new(AGGREGATE_ID),
            }
        );
        Ok(())
    })
    .expect("test model should be valid");
    assert_eq!(model["aggregates"].as_array().unwrap().len(), 1);
}

#[test]
fn entity_and_identity_insertion_is_atomic_with_entity_precedence() {
    let borrowed_identity = EntityDescriptor {
        id: EntityId {
            aggregate: AGGREGATE_ID,
            local: "other",
        },
        label: "Other",
        identity: Root::DESCRIPTOR.identity,
        fields: &[],
    };
    let model = empty_model_with(|builder| {
        builder.add_entity(borrowed_identity)?;
        assert_eq!(
            builder
                .add_entity(Root::DESCRIPTOR)
                .expect_err("duplicate identity must fail"),
            DomainModelError::DuplicateDomainIdentityId {
                id: Box::new(Root::DESCRIPTOR.identity),
            }
        );
        Ok(())
    })
    .expect("test model should be valid");
    assert_eq!(model["entities"].as_array().unwrap().len(), 1);
    assert_eq!(model["domainIdentities"].as_array().unwrap().len(), 1);

    assert_eq!(
        domain_model! { contexts: [], aggregates: [], entities: [Root, Root], value_objects: [], services: [], errors: [] }
            .expect_err("entity collision must take precedence"),
        DomainModelError::DuplicateEntityId {
            id: Box::new(ROOT_ID),
        }
    );
}

#[test]
fn duplicate_event_descriptors_remain_atomic() {
    let descriptor = event_descriptor("First event");
    let mut builder = domain::__private::DomainModelBuilder::new();
    builder.add_domain_event(descriptor).expect("first event");
    assert_eq!(
        builder
            .add_domain_event(descriptor)
            .expect_err("duplicate event must fail"),
        DomainModelError::DuplicateDomainEventId {
            id: Box::new(descriptor.id),
        }
    );
    let model = builder.finish().expect("failed insertion must be atomic");
    assert_eq!(model["domainEvents"].as_array().unwrap().len(), 1);
}
