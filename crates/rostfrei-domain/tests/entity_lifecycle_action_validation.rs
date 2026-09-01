use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOwnerId, ActionOwnerType,
    AggregateDescriptor, AggregateId, AggregateType, BoundedContextDescriptor, BoundedContextId,
    BoundedContextType, DecisionDescriptor, DecisionId, DecisionImplementationDescriptor,
    DecisionInputDescriptor, DecisionOutcomeDescriptor, DecisionOutcomeShapeDescriptor,
    DecisionOwnerId, DecisionParameterDescriptor, DomainIdentityDescriptor, DomainIdentityId,
    DomainIdentityType, DomainModelError, DomainModelReference, EntityDescriptor, EntityId,
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleTransitionDescriptor, EntityType, IdentityDescriptor,
    ScalarType, ValueObjectId, ValueObjectOwnerId,
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
    raises: &[],
    error: None,
}];
const ATTACHED_CONTRACTS: &[&[ActionDescriptor]] = &[ATTACHED_ACTIONS];
const BROKEN_ACTIONS: &[ActionDescriptor] = &[ActionDescriptor {
    id: BROKEN_ACTION_ID,
    label: "Broken reference",
    input: Some(ActionInputDescriptor::ValueObject(MISSING_VALUE_ID)),
    output: None,
    raises: &[],
    error: None,
}];
const BROKEN_ACTION_CONTRACTS: &[&[ActionDescriptor]] = &[BROKEN_ACTIONS];
const EXTENSION_ACTIONS: &[ActionDescriptor] = &[ActionDescriptor {
    id: EXTENSION_ID,
    label: "Extension only",
    input: None,
    output: None,
    raises: &[],
    error: None,
}];
const BROKEN_DECISIONS: &[DecisionDescriptor] = &[DecisionDescriptor {
    id: DecisionId {
        owner: DecisionOwnerId::Entity(ENTITY_ID),
        local: "broken-decision",
    },
    label: "Broken decision",
    parameters: &[DecisionParameterDescriptor {
        name: "input",
        input: DecisionInputDescriptor::ValueObject(MISSING_VALUE_ID),
    }],
    outcomes: &[DecisionOutcomeDescriptor {
        local_id: "done",
        label: "Done",
        shape: DecisionOutcomeShapeDescriptor::Unit,
    }],
    implementation: DecisionImplementationDescriptor::Rust,
}];
const BROKEN_DECISION_GROUPS: &[&[DecisionDescriptor]] = &[BROKEN_DECISIONS];

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

const fn decision_groups(case: u8) -> &'static [&'static [DecisionDescriptor]] {
    match case {
        5 => BROKEN_DECISION_GROUPS,
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
    const DESCRIPTOR: AggregateDescriptor = AggregateDescriptor {
        id: AGGREGATE_ID,
        label: "Aggregate",
        root: ENTITY_ID,
    };
}

impl domain::AggregateDefinition for ValidationAggregate {
    type Context = ValidationContext;
    type Root = ValidationEntity<0>;
    type Event = domain::NoDomainEvents;
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
    const DECISION_GROUPS: &'static [&'static [DecisionDescriptor]] = decision_groups(CASE);
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

fn finish<const CASE: u8>() -> Result<serde_json::Value, DomainModelError> {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<ValidationEntity<CASE>>()?;
    builder.finish()
}

#[test]
fn accepts_an_action_from_a_normally_attached_contract() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<ValidationEntity<0>>().unwrap();

    let model = builder.finish().unwrap();

    assert_eq!(
        model["entities"][0]["lifecycle"]["transitions"][0]["action"]["local"],
        "attached"
    );
}

#[test]
fn rejects_an_implemented_but_unattached_action_as_missing() {
    let entity = ValidationEntity::<1>;
    entity.implemented_but_unattached();

    let error = finish::<1>().unwrap_err();
    let message = error.to_string();

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("implemented-unattached"));
    assert_eq!(
        error,
        DomainModelError::LifecycleMissingAttachedAction {
            lifecycle_id: Box::new(LIFECYCLE_ID),
            action_id: Box::new(IMPLEMENTED_UNATTACHED_ID),
        }
    );
}

#[test]
fn rejects_a_fabricated_action_as_missing() {
    let error = finish::<2>().unwrap_err();
    let message = error.to_string();

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("fabricated"));
    assert_eq!(
        error,
        DomainModelError::LifecycleMissingAttachedAction {
            lifecycle_id: Box::new(LIFECYCLE_ID),
            action_id: Box::new(FABRICATED_ID),
        }
    );
}

#[test]
fn rejects_an_extension_only_action_with_a_distinct_diagnostic() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<ValidationEntity<3>>().unwrap();
    builder
        .add_action_extension::<LifecycleExtension>()
        .unwrap();

    let error = builder.finish().unwrap_err();
    let message = error.to_string();

    assert!(message.starts_with("Entity lifecycle action eligibility violation"));
    assert!(message.contains("extension-only action"));
    assert!(message.contains("action extensions are not eligible"));
    assert_eq!(
        error,
        DomainModelError::LifecycleExtensionOnlyAction {
            lifecycle_id: Box::new(LIFECYCLE_ID),
            action_id: Box::new(EXTENSION_ID),
        }
    );
}

#[test]
fn validates_existing_action_descriptor_references_before_lifecycle_actions() {
    let error = finish::<4>().unwrap_err();
    let message = error.to_string();

    assert!(message.starts_with("Action reference inventory violation"));
    assert!(message.contains("broken-reference"));
    assert!(message.contains("missing-value"));
    assert_eq!(
        error,
        DomainModelError::ActionReferenceInventoryViolation {
            action_id: Box::new(BROKEN_ACTION_ID),
            reference: DomainModelReference::ValueObject(Box::new(MISSING_VALUE_ID)),
            location: "input".to_owned(),
            inventory_key: "value_objects",
        }
    );
}

#[test]
fn validates_lifecycle_actions_before_existing_decision_references() {
    let error = finish::<5>().unwrap_err();
    let message = error.to_string();

    assert!(message.starts_with("Entity lifecycle action inventory violation"));
    assert!(message.contains("fabricated"));
    assert_eq!(
        error,
        DomainModelError::LifecycleMissingAttachedAction {
            lifecycle_id: Box::new(LIFECYCLE_ID),
            action_id: Box::new(FABRICATED_ID),
        }
    );
}

#[test]
fn reports_missing_actions_in_transition_order() {
    let error = finish::<6>().unwrap_err();
    let message = error.to_string();

    assert!(message.contains("ordered-first"));
    assert!(!message.contains("ordered-second"));
    assert_eq!(
        error,
        DomainModelError::LifecycleMissingAttachedAction {
            lifecycle_id: Box::new(LIFECYCLE_ID),
            action_id: Box::new(ORDERED_FIRST_ID),
        }
    );
}
