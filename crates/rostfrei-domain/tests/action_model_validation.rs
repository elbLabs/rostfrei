#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, Aggregate, AggregateId, BoundedContext,
    BoundedContextId, DomainIdentity, Entity, EntityId, domain_actions,
};

const BOUNDARY_AGGREGATE_ID: AggregateId = AggregateId {
    context: BoundedContextId("validation"),
    local: "boundary",
};
const BOUNDARY_ENTITY_ID: EntityId = EntityId {
    aggregate: BOUNDARY_AGGREGATE_ID,
    local: "boundary-entity",
};

const fn action(
    owner: ActionOwnerId,
    local: &'static str,
    label: &'static str,
) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId { owner, local },
        label,
        input: None,
        output: None,
        raises: &[],
        error: None,
    }
}

#[derive(BoundedContext)]
#[domain(id = "validation", label = "Validation")]
struct Validation;

#[derive(DomainIdentity)]
#[domain(owner = BoundaryEntity)]
struct BoundaryEntityId(u64);

#[domain_actions(entity)]
trait AttachedActions {
    #[action(id = "shared", label = "Attached")]
    fn attached(&self);
}

#[derive(Entity)]
#[domain(
    id = "boundary-entity",
    label = "Boundary entity",
    owner = BoundaryAggregate,
    actions = [AttachedActions]
)]
struct BoundaryEntity {
    #[domain(identity)]
    id: BoundaryEntityId,
}

impl AttachedActions for BoundaryEntity {
    fn attached(&self) {}
}

#[derive(Aggregate)]
#[domain(
    id = "boundary",
    label = "Boundary",
    context = Validation,
    root = BoundaryEntity
)]
struct BoundaryAggregate;

struct WrongOwnerExtension;

impl ActionGroupType for WrongOwnerExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Aggregate(BOUNDARY_AGGREGATE_ID),
        "wrong",
        "Wrong",
    )];
}

struct FirstExtension;

impl ActionGroupType for FirstExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Entity(BOUNDARY_ENTITY_ID),
        "first",
        "First",
    )];
}

struct SecondExtension;

impl ActionGroupType for SecondExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Entity(BOUNDARY_ENTITY_ID),
        "second",
        "Second",
    )];
}

struct FirstDuplicateExtension;

impl ActionGroupType for FirstDuplicateExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Entity(BOUNDARY_ENTITY_ID),
        "duplicate",
        "First duplicate",
    )];
}

struct SecondDuplicateExtension;

impl ActionGroupType for SecondDuplicateExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Entity(BOUNDARY_ENTITY_ID),
        "duplicate",
        "Second duplicate",
    )];
}

struct AttachedDuplicateExtension;

impl ActionGroupType for AttachedDuplicateExtension {
    type Owner = BoundaryEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Entity(BOUNDARY_ENTITY_ID),
        "shared",
        "Extension duplicate",
    )];
}

#[derive(DomainIdentity)]
#[domain(owner = DuplicateTraitEntity)]
struct DuplicateTraitEntityId(u64);

#[domain_actions(entity)]
trait FirstDuplicateActions {
    #[action(id = "duplicate", label = "First duplicate")]
    fn first(&self);
}

#[domain_actions(entity)]
trait SecondDuplicateActions {
    #[action(id = "duplicate", label = "Second duplicate")]
    fn second(&self);
}

#[derive(Entity)]
#[domain(
    id = "duplicate-trait-entity",
    label = "Duplicate trait entity",
    owner = BoundaryAggregate,
    actions = [FirstDuplicateActions, SecondDuplicateActions]
)]
struct DuplicateTraitEntity {
    #[domain(identity)]
    id: DuplicateTraitEntityId,
}

impl FirstDuplicateActions for DuplicateTraitEntity {
    fn first(&self) {}
}

impl SecondDuplicateActions for DuplicateTraitEntity {
    fn second(&self) {}
}

#[test]
#[should_panic(expected = "unregistered action extension owner")]
fn rejects_extension_for_an_unregistered_owner() {
    let mut builder = DomainModelBuilder::new();
    builder.add_action_extension::<FirstExtension>();
}

#[test]
#[should_panic(expected = "action descriptor owner mismatch")]
fn rejects_extension_descriptor_owner_mismatch() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<BoundaryEntity>();
    builder.add_action_extension::<WrongOwnerExtension>();
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_across_extensions() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<BoundaryEntity>();
    builder.add_action_extension::<FirstDuplicateExtension>();
    builder.add_action_extension::<SecondDuplicateExtension>();
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_against_an_attached_contract() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<BoundaryEntity>();
    builder.add_action_extension::<AttachedDuplicateExtension>();
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_across_entity_attached_traits_during_model_registration() {
    let _ = domain::domain_model! {
        contexts: [],
        aggregates: [],
        entities: [DuplicateTraitEntity],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],

        query_groups: [],
    };
}

#[test]
fn preserves_multiple_extensions_for_the_same_registered_owner() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<BoundaryEntity>();
    builder.add_domain_identity_type::<BoundaryEntityId>();
    builder.add_action_extension::<FirstExtension>();
    builder.add_action_extension::<SecondExtension>();

    let model = builder.finish();
    let actions = model["actions"].as_array().unwrap();

    assert_eq!(actions.len(), 3);
    assert_eq!(actions[0]["id"]["local"], "shared");
    assert_eq!(actions[1]["id"]["local"], "first");
    assert_eq!(actions[2]["id"]["local"], "second");
    assert!(actions.iter().all(|action| {
        action["id"]["owner"]["kind"] == "entity"
            && action["id"]["owner"]["id"]["local"] == "boundary-entity"
    }));
}
