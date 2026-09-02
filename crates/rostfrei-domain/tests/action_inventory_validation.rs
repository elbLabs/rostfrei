#![allow(
    dead_code,
    clippy::expect_used,
    reason = "test assertions require expected outcomes"
)]

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, Aggregate, AggregateType, BoundedContext,
    DomainErrorId, DomainErrorOwnerId, DomainIdentity, Entity,
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
    type Event = domain::NoDomainEvents;
}

struct MissingError;

impl ActionGroupType for MissingError {
    type Owner = Owner;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Owner::DESCRIPTOR.id),
            local: "missing-error",
        },
        label: "Missing error",
        error: Some(DomainErrorId {
            owner: DomainErrorOwnerId::Aggregate(Owner::DESCRIPTOR.id),
            local: "missing",
        }),
    }];
}

#[test]
fn rejects_missing_domain_error() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<Owner>()
        .expect("fixture aggregate should register");
    builder
        .add_action_extension::<MissingError>()
        .expect("fixture action extension should register");

    let error = builder.finish().unwrap_err();
    assert!(matches!(
        error,
        domain::DomainModelError::ActionErrorInventoryViolation { .. }
    ));
    assert!(
        error
            .to_string()
            .contains("references missing DomainErrorId")
    );
}
