#![allow(
    dead_code,
    clippy::expect_used,
    reason = "test assertions require expected outcomes"
)]

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, Aggregate, AggregateType, BoundedContext,
    DomainErrorId, DomainErrorOwnerId, DomainEvent, DomainEventId, DomainIdentity, Entity,
};

#[derive(BoundedContext)]
#[domain(id = "validation", label = "Validation")]
struct Validation;

#[derive(DomainIdentity)]
struct RootId;

#[derive(Entity)]
#[domain(id = "root", label = "Root")]
struct Root {
    #[domain(identity)]
    id: RootId,
}

impl domain::EntityDefinition for Root {
    type Owner = Owner;
    type Identity = RootId;
}

#[derive(Aggregate)]
#[domain(id = "owner", label = "Owner")]
struct Owner;

impl domain::AggregateDefinition for Owner {
    type Context = Validation;
    type Root = Root;
    type Event = OwnerEvents;
}

#[derive(DomainEvent)]
#[domain(id = "registered", label = "Registered")]
struct Registered;

#[derive(domain::AggregateEvents)]
enum OwnerEvents {
    Registered(Registered),
}

const fn action(
    local: &'static str,
    raises: &'static [DomainEventId],
    error: Option<DomainErrorId>,
) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Owner::DESCRIPTOR.id),
            local,
        },
        label: local,
        raises,
        error,
    }
}

struct MissingEvent;

impl ActionGroupType for MissingEvent {
    type Owner = Owner;
    const ACTIONS: &'static [ActionDescriptor] = &[action(
        "missing-event",
        &[DomainEventId {
            aggregate: Owner::DESCRIPTOR.id,
            local: "missing",
        }],
        None,
    )];
}

struct ForeignEvent;

impl ActionGroupType for ForeignEvent {
    type Owner = Owner;
    const ACTIONS: &'static [ActionDescriptor] = &[action(
        "foreign-event",
        &[DomainEventId {
            aggregate: domain::AggregateId {
                context: <Validation as domain::BoundedContextType>::DESCRIPTOR.id,
                local: "foreign",
            },
            local: "changed",
        }],
        None,
    )];
}

struct MissingError;

impl ActionGroupType for MissingError {
    type Owner = Owner;
    const ACTIONS: &'static [ActionDescriptor] = &[action(
        "missing-error",
        &[],
        Some(DomainErrorId {
            owner: DomainErrorOwnerId::Aggregate(Owner::DESCRIPTOR.id),
            local: "missing",
        }),
    )];
}

fn builder_with<G: ActionGroupType<Owner = Owner>>() -> DomainModelBuilder {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<Owner>()
        .expect("fixture aggregate should register");
    builder
        .add_action_extension::<G>()
        .expect("fixture action extension should register");
    builder
}

#[test]
fn rejects_missing_raised_event() {
    let error = builder_with::<MissingEvent>().finish().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("references missing DomainEventId")
    );
}

#[test]
fn rejects_cross_owner_raised_event() {
    let error = builder_with::<ForeignEvent>().finish().unwrap_err();
    assert!(matches!(
        error,
        domain::DomainModelError::ActionRaisedEventOwnerMismatch { .. }
    ));
}

#[test]
fn rejects_missing_domain_error() {
    let error = builder_with::<MissingError>().finish().unwrap_err();
    assert!(
        error
            .to_string()
            .contains("references missing DomainErrorId")
    );
}
