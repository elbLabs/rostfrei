#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, ActionOwnerType, Aggregate, AggregateId,
    BoundedContext, BoundedContextId, DomainIdentity, DomainModelError, DomainService,
    DomainServiceId, Entity, EntityId, domain_actions, domain_model,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("extensions");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "extension-owner",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "extension-root",
};
const SERVICE_ID: DomainServiceId = DomainServiceId {
    context: CONTEXT_ID,
    local: "extension-service",
};
const UNREGISTERED_OWNER_ID: ActionOwnerId = ActionOwnerId::Aggregate(AggregateId {
    context: CONTEXT_ID,
    local: "unregistered",
});

const fn action(owner: ActionOwnerId, local: &'static str) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId { owner, local },
        label: local,
        error: None,
    }
}

#[derive(BoundedContext)]
#[domain(id = "extensions", label = "Extensions")]
pub struct Extensions;

#[derive(DomainIdentity)]
pub struct ExtensionRootId(u64);

#[derive(Entity)]
#[domain(id = "extension-root", label = "Extension root")]
pub struct ExtensionRoot {
    #[domain(identity)]
    id: ExtensionRootId,
}

impl domain::EntityDefinition for ExtensionRoot {
    type Owner = ExtensionOwner;
    type Identity = ExtensionRootId;
}

#[domain_actions(aggregate)]
pub trait AttachedActions {
    #[action(id = "attached", label = "Attached")]
    fn attached(root: &mut ExtensionRoot);
}

#[derive(Aggregate)]
#[domain(id = "extension-owner", label = "Extension owner")]
pub struct ExtensionOwner;

impl domain::AggregateDefinition for ExtensionOwner {
    type Context = Extensions;
    type Root = ExtensionRoot;
    type Event = domain::NoDomainEvents;
}

impl AttachedActions for ExtensionOwner {
    fn attached(_root: &mut ExtensionRoot) {}
}

#[derive(DomainService)]
#[domain(
    id = "extension-service",
    label = "Extension service",
    context = Extensions
)]
struct ExtensionService;

struct FirstAggregateExtension;

impl ActionGroupType for FirstAggregateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[
        action(ActionOwnerId::Aggregate(AGGREGATE_ID), "extension-one"),
        action(ActionOwnerId::Aggregate(AGGREGATE_ID), "extension-two"),
    ];
}

struct SecondAggregateExtension;

impl ActionGroupType for SecondAggregateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Aggregate(AGGREGATE_ID),
        "extension-three",
    )];
}

struct EntityExtension;

impl ActionGroupType for EntityExtension {
    type Owner = ExtensionRoot;

    const ACTIONS: &'static [ActionDescriptor] =
        &[action(ActionOwnerId::Entity(ENTITY_ID), "entity-extension")];
}

struct ServiceExtension;

impl ActionGroupType for ServiceExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::DomainService(SERVICE_ID),
        "service-extension",
    )];
}

struct WrongOwnerExtension;

impl ActionGroupType for WrongOwnerExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] =
        &[action(ActionOwnerId::Entity(ENTITY_ID), "wrong-owner")];
}

struct UnregisteredOwner;

impl ActionOwnerType for UnregisteredOwner {
    const ACTION_OWNER_ID: ActionOwnerId = UNREGISTERED_OWNER_ID;
}

struct UnregisteredOwnerExtension;

impl ActionGroupType for UnregisteredOwnerExtension {
    type Owner = UnregisteredOwner;

    const ACTIONS: &'static [ActionDescriptor] =
        &[action(UNREGISTERED_OWNER_ID, "unregistered-extension")];
}

struct EmptyExtension;

impl ActionGroupType for EmptyExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[];
}

struct FirstDuplicateExtension;

impl ActionGroupType for FirstDuplicateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Aggregate(AGGREGATE_ID),
        "duplicate-extension",
    )];
}

struct SecondDuplicateExtension;

impl ActionGroupType for SecondDuplicateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[action(
        ActionOwnerId::Aggregate(AGGREGATE_ID),
        "duplicate-extension",
    )];
}

struct SingleSliceDuplicateExtension;

impl ActionGroupType for SingleSliceDuplicateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] = &[
        action(ActionOwnerId::Aggregate(AGGREGATE_ID), "duplicate-in-slice"),
        action(ActionOwnerId::Aggregate(AGGREGATE_ID), "duplicate-in-slice"),
    ];
}

struct AttachedDuplicateExtension;

impl ActionGroupType for AttachedDuplicateExtension {
    type Owner = ExtensionOwner;

    const ACTIONS: &'static [ActionDescriptor] =
        &[action(ActionOwnerId::Aggregate(AGGREGATE_ID), "attached")];
}

fn action_locals(model: &serde_json::Value) -> Result<Vec<&str>, &'static str> {
    model["actions"]
        .as_array()
        .ok_or("domain model actions should be an array")?
        .iter()
        .map(|action| {
            action
                .pointer("/id/local")
                .and_then(serde_json::Value::as_str)
                .ok_or("domain action should have a string ID local part")
        })
        .collect()
}

#[test]
fn accepts_extensions_for_every_registered_action_owner_kind() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");
    builder
        .add_entity_type::<ExtensionRoot>()
        .expect("extension root should register");
    builder
        .add_domain_service_type::<ExtensionService>()
        .expect("extension service should register");
    builder
        .add_action_extension::<FirstAggregateExtension>()
        .expect("aggregate extension should register");
    builder
        .add_action_extension::<EntityExtension>()
        .expect("entity extension should register");
    builder
        .add_action_extension::<ServiceExtension>()
        .expect("service extension should register");

    let model = builder.finish().expect("domain model should be valid");

    assert_eq!(
        action_locals(&model).unwrap(),
        [
            "extension-one",
            "extension-two",
            "entity-extension",
            "service-extension",
        ]
    );
}

#[test]
fn extensions_follow_attached_contracts_and_preserve_extension_order() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");
    builder
        .add_action_extension::<FirstAggregateExtension>()
        .expect("first extension should register");
    builder
        .add_action_extension::<SecondAggregateExtension>()
        .expect("second extension should register");

    let model = builder.finish().expect("domain model should be valid");

    assert_eq!(
        action_locals(&model).unwrap(),
        ["extension-one", "extension-two", "extension-three",]
    );
}

#[test]
fn rejects_extension_descriptor_owner_mismatch() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");

    let id = ActionId {
        owner: ActionOwnerId::Entity(ENTITY_ID),
        local: "wrong-owner",
    };
    let error = builder
        .add_action_extension::<WrongOwnerExtension>()
        .expect_err("mismatched extension owner should be rejected");

    assert_eq!(
        error,
        DomainModelError::ActionDescriptorOwnerMismatch { id: Box::new(id) }
    );
    assert_eq!(
        error.to_string(),
        format!("action descriptor owner mismatch: {id:?}")
    );
}

#[test]
fn rejects_extension_for_an_unregistered_owner() {
    let mut builder = DomainModelBuilder::new();
    let error = builder
        .add_action_extension::<UnregisteredOwnerExtension>()
        .expect_err("an extension for an unregistered owner should be rejected");

    assert_eq!(
        error,
        DomainModelError::UnregisteredActionExtensionOwner {
            owner: Box::new(UNREGISTERED_OWNER_ID),
        }
    );
    assert_eq!(
        error.to_string(),
        format!("unregistered action extension owner: {UNREGISTERED_OWNER_ID:?}")
    );
}

#[test]
fn rejects_empty_extension() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");
    let error = builder
        .add_action_extension::<EmptyExtension>()
        .expect_err("an empty extension should be rejected");

    assert_eq!(error, DomainModelError::EmptyActionExtension);
    assert_eq!(error.to_string(), "action extension must not be empty");
}

#[test]
fn rejects_duplicate_action_id_across_extensions() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");
    builder
        .add_action_extension::<FirstDuplicateExtension>()
        .expect("first duplicate extension should register");

    let id = ActionId {
        owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
        local: "duplicate-extension",
    };
    let error = builder
        .add_action_extension::<SecondDuplicateExtension>()
        .expect_err("the duplicate extension action should be rejected");

    assert_eq!(
        error,
        DomainModelError::DuplicateActionId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate ActionId: {id:?}"));
}

#[test]
fn rejects_duplicate_action_id_within_one_extension_slice() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");

    let id = ActionId {
        owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
        local: "duplicate-in-slice",
    };
    let error = builder
        .add_action_extension::<SingleSliceDuplicateExtension>()
        .expect_err("a duplicate action within one extension should be rejected");

    assert_eq!(
        error,
        DomainModelError::DuplicateActionId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate ActionId: {id:?}"));
}

#[test]
fn permits_an_extension_when_the_same_unattached_contract_is_not_registered() {
    let mut builder = DomainModelBuilder::new();
    builder
        .add_aggregate_type::<ExtensionOwner>()
        .expect("extension owner should register");

    builder
        .add_action_extension::<AttachedDuplicateExtension>()
        .expect("unattached contracts do not participate in model registration");
}

#[test]
fn domain_model_accepts_optional_action_extensions_and_still_allows_omission() {
    let extended = domain_model! {
        contexts: [],
        aggregates: [ExtensionOwner],
        entities: [],
        value_objects: [],
        services: [],
        errors: [],
        action_extensions: [FirstAggregateExtension, SecondAggregateExtension],
        query_groups: [],
    }
    .expect("extended domain model should be valid");
    let omitted = domain_model! {
        contexts: [],
        aggregates: [ExtensionOwner],
        entities: [],
        value_objects: [],
        services: [],
        errors: [],
        query_groups: [],
    }
    .expect("domain model without extensions should be valid");

    assert_eq!(
        action_locals(&extended).unwrap(),
        ["extension-one", "extension-two", "extension-three",]
    );
    assert!(action_locals(&omitted).unwrap().is_empty());
}
