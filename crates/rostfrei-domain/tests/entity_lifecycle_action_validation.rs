use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOwnerId, ActionOwnerType,
    AggregateDescriptor, AggregateId, AggregateType, BoundedContextDescriptor, BoundedContextId,
    BoundedContextType, DecisionDescriptor, DecisionId, DecisionImplementationDescriptor,
    DecisionInputDescriptor, DecisionOutputDescriptor, DecisionOwnerId, DomainIdentityDescriptor,
    DomainIdentityId, DomainIdentityType, EntityDescriptor, EntityId, EntityLifecycleDescriptor,
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId,
    EntityLifecycleTransitionDescriptor, EntityType, IdentityDescriptor, ScalarType, ValueObjectId,
    ValueObjectOwnerId,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("lifecycle-action-validation");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "aggregate",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "entity",
};
const IDENTITY_ID: DomainIdentityId = DomainIdentityId { owner: ENTITY_ID };
const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "workflow",
};
const READY_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "ready",
};
const ATTACHED_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "attached",
};
const IMPLEMENTED_UNATTACHED_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "implemented-unattached",
};
const FABRICATED_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "fabricated",
};
const EXTENSION_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "extension-only",
};
const ORDERED_FIRST_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "ordered-first",
};
const ORDERED_SECOND_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "ordered-second",
};
const BROKEN_ACTION_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "broken-reference",
};
const MISSING_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Entity(ENTITY_ID),
    local: "missing-value",
};
const ATTACHED_ACTIONS: &[ActionDescriptor] = &[ActionDescriptor {
    id: ATTACHED_ID,
    label: "Attached",
    input: None,
    output: None,
    error: None,
}];
const ATTACHED_CONTRACTS: &[&[ActionDescriptor]] = &[ATTACHED_ACTIONS];
const BROKEN_ACTIONS: &[ActionDescriptor] = &[ActionDescriptor {
    id: BROKEN_ACTION_ID,
    label: "Broken reference",
    input: Some(ActionInputDescriptor::ValueObject(MISSING_VALUE_ID)),
    output: None,
    error: None,
}];
const BROKEN_ACTION_CONTRACTS: &[&[ActionDescriptor]] = &[BROKEN_ACTIONS];
const EXTENSION_ACTIONS: &[ActionDescriptor] = &[ActionDescriptor {
    id: EXTENSION_ID,
    label: "Extension only",
    input: None,
    output: None,
    error: None,
}];
const BROKEN_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DecisionId {
        owner: DecisionOwnerId::Entity(ENTITY_ID),
        local: "broken-decision",
    },
    label: "Broken decision",
    input: DecisionInputDescriptor::ValueObject(MISSING_VALUE_ID),
    output: DecisionOutputDescriptor::ValueObject(MISSING_VALUE_ID),
    implementation: DecisionImplementationDescriptor::Rust,
}];
const BROKEN_DECISION_CONTRACTS: &[&[DecisionDescriptor]] = &[BROKEN_DECISIONS];

const STATES: &[EntityLifecycleStateDescriptor] = &[EntityLifecycleStateDescriptor {
    id: READY_ID,
    label: "Ready",
}];
const ATTACHED_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: ATTACHED_ID,
        target: READY_ID,
    }];
const IMPLEMENTED_UNATTACHED_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: IMPLEMENTED_UNATTACHED_ID,
        target: READY_ID,
    }];
const FABRICATED_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: FABRICATED_ID,
        target: READY_ID,
    }];
const EXTENSION_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: EXTENSION_ID,
        target: READY_ID,
    }];
const ORDERED_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] = &[
    EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: ORDERED_FIRST_ID,
        target: READY_ID,
    },
    EntityLifecycleTransitionDescriptor {
        source: READY_ID,
        action: ORDERED_SECOND_ID,
        target: READY_ID,
    },
];

const fn lifecycle_with(
    transitions: &'static [EntityLifecycleTransitionDescriptor],
) -> EntityLifecycleDescriptor {
    EntityLifecycleDescriptor {
        id: LIFECYCLE_ID,
        label: "Workflow",
        states: STATES,
        initial: READY_ID,
        transitions,
    }
}

const fn lifecycle(case: u8) -> EntityLifecycleDescriptor {
    match case {
        1 => lifecycle_with(IMPLEMENTED_UNATTACHED_TRANSITIONS),
        2 | 4 | 5 => lifecycle_with(FABRICATED_TRANSITIONS),
        3 => lifecycle_with(EXTENSION_TRANSITIONS),
        6 => lifecycle_with(ORDERED_TRANSITIONS),
        _ => lifecycle_with(ATTACHED_TRANSITIONS),
    }
}

const fn action_contracts(case: u8) -> &'static [&'static [ActionDescriptor]] {
    match case {
        0 => ATTACHED_CONTRACTS,
        4 => BROKEN_ACTION_CONTRACTS,
        _ => &[],
    }
}

const fn decision_contracts(case: u8) -> &'static [&'static [DecisionDescriptor]] {
    match case {
        5 => BROKEN_DECISION_CONTRACTS,
        _ => &[],
    }
}

struct ValidationContext;

impl BoundedContextType for ValidationContext {
    const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
        id: CONTEXT_ID,
        label: "Lifecycle action validation",
    };
}

struct ValidationAggregate;

impl AggregateType for ValidationAggregate {
    type Context = ValidationContext;
    type Root = ValidationEntity<0>;

    const DESCRIPTOR: AggregateDescriptor = AggregateDescriptor {
        id: AGGREGATE_ID,
        label: "Aggregate",
        root: ENTITY_ID,
    };
}

struct ValidationEntity<const CASE: u8>;

impl<const CASE: u8> EntityType for ValidationEntity<CASE> {
    type Owner = ValidationAggregate;
    type Identity = ValidationIdentity<CASE>;

    const LOCAL_ID: &'static str = "entity";
    const DESCRIPTOR: EntityDescriptor = EntityDescriptor {
        id: ENTITY_ID,
        label: "Entity",
        identity: IdentityDescriptor {
            field: "id",
            identity: IDENTITY_ID,
        },
        fields: &[],
    };
    const LIFECYCLE: Option<EntityLifecycleDescriptor> = Some(lifecycle(CASE));
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = action_contracts(CASE);
    const DECISION_CONTRACTS: &'static [&'static [DecisionDescriptor]] = decision_contracts(CASE);
}

impl<const CASE: u8> ActionOwnerType for ValidationEntity<CASE> {
    const ACTION_OWNER_ID: ActionOwnerId = ActionOwnerId::Entity(ENTITY_ID);
}

struct ValidationIdentity<const CASE: u8>;

impl<const CASE: u8> DomainIdentityType for ValidationIdentity<CASE> {
    type Owner = ValidationEntity<CASE>;

    const DESCRIPTOR: DomainIdentityDescriptor = DomainIdentityDescriptor {
        id: IDENTITY_ID,
        scalar: ScalarType::U64,
    };
}

trait ImplementedButUnattached {
    fn implemented_but_unattached(&self);
}

impl ImplementedButUnattached for ValidationEntity<1> {
    fn implemented_but_unattached(&self) {}
}

struct LifecycleExtension;

impl ActionGroupType for LifecycleExtension {
    type Owner = ValidationEntity<3>;

    const ACTIONS: &'static [ActionDescriptor] = EXTENSION_ACTIONS;
}

fn finish_panic<const CASE: u8>(configure: impl FnOnce(&mut DomainModelBuilder)) -> String {
    let payload = catch_unwind(AssertUnwindSafe(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<ValidationEntity<CASE>>();
        configure(&mut builder);
        builder.finish();
    }))
    .expect_err("finish should panic");
    panic_payload(payload)
}

fn panic_payload(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload.downcast::<&'static str>().map_or_else(
            |_| panic!("panic payload should be a String or &'static str"),
            |message| (*message).to_owned(),
        ),
    }
}

#[test]
fn accepts_an_action_from_a_normally_attached_contract() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<ValidationEntity<0>>();

    let model = builder.finish();

    assert_eq!(
        model["entities"][0]["lifecycle"]["transitions"][0]["action"]["local"],
        "attached"
    );
}

#[test]
fn rejects_an_implemented_but_unattached_action_as_missing() {
    let entity = ValidationEntity::<1>;
    entity.implemented_but_unattached();

    let message = finish_panic::<1>(|_| {});

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("implemented-unattached"));
}

#[test]
fn rejects_a_fabricated_action_as_missing() {
    let message = finish_panic::<2>(|_| {});

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("fabricated"));
}

#[test]
fn rejects_an_extension_only_action_with_a_distinct_diagnostic() {
    let message = finish_panic::<3>(|builder| {
        builder.add_action_extension::<LifecycleExtension>();
    });

    assert!(message.starts_with("Entity lifecycle action eligibility violation"));
    assert!(message.contains("extension-only action"));
    assert!(message.contains("action extensions are not eligible"));
}

#[test]
fn validates_existing_action_descriptor_references_before_lifecycle_actions() {
    let message = finish_panic::<4>(|_| {});

    assert!(message.starts_with("Action reference inventory violation"));
    assert!(message.contains("broken-reference"));
    assert!(message.contains("missing-value"));
}

#[test]
fn validates_lifecycle_actions_before_existing_decision_references() {
    let message = finish_panic::<5>(|_| {});

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("fabricated"));
}

#[test]
fn reports_missing_actions_in_transition_order() {
    let message = finish_panic::<6>(|_| {});

    assert!(message.contains("ordered-first"));
    assert!(!message.contains("ordered-second"));
}
