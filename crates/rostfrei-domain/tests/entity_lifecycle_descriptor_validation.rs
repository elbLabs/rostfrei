use domain::__private::DomainModelBuilder;
use domain::{
    ActionId, ActionOwnerId, AggregateDescriptor, AggregateId, AggregateType,
    BoundedContextDescriptor, BoundedContextId, BoundedContextType, DomainIdentityDescriptor,
    DomainIdentityId, DomainIdentityType, DomainModelError, EntityDescriptor, EntityId,
    EntityLifecycleDescriptor, EntityLifecycleId, EntityLifecycleStateDescriptor,
    EntityLifecycleStateId, EntityLifecycleTransitionDescriptor, EntityType, IdentityDescriptor,
    ScalarType,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("lifecycle-descriptor-validation");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "aggregate",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "entity",
};
const FOREIGN_ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "foreign",
};
const IDENTITY_ID: DomainIdentityId = DomainIdentityId { owner: ENTITY_ID };
const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "workflow",
};
const FOREIGN_LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "foreign-workflow",
};
const FOREIGN_OWNER_LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: FOREIGN_ENTITY_ID,
    local: "Workflow",
};
const DRAFT_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "draft",
};
const ACTIVE_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "active",
};
const MISSING_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "missing",
};
const FOREIGN_DRAFT_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: FOREIGN_LIFECYCLE_ID,
    local: "draft",
};
const ACTION_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "advance",
};
const WRONG_OWNER_ACTION_ID: ActionId = ActionId {
    owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
    local: "advance",
};
const STATES: &[EntityLifecycleStateDescriptor] = &[
    EntityLifecycleStateDescriptor {
        id: DRAFT_ID,
        label: "Draft",
    },
    EntityLifecycleStateDescriptor {
        id: ACTIVE_ID,
        label: "Active",
    },
];
const VALID_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: ACTION_ID,
        target: ACTIVE_ID,
    }];
const VALID: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    id: LIFECYCLE_ID,
    label: "Workflow",
    states: STATES,
    initial: DRAFT_ID,
    transitions: VALID_TRANSITIONS,
};
const WRONG_OWNER: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    id: FOREIGN_OWNER_LIFECYCLE_ID,
    label: "   ",
    states: &[],
    initial: EntityLifecycleStateId {
        lifecycle: FOREIGN_OWNER_LIFECYCLE_ID,
        local: "draft",
    },
    transitions: &[],
};
const EMPTY_STATES: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[],
    transitions: &[],
    ..VALID
};
const WRONG_STATE_LIFECYCLE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[EntityLifecycleStateDescriptor {
        id: FOREIGN_DRAFT_ID,
        label: "Draft",
    }],
    initial: FOREIGN_DRAFT_ID,
    transitions: &[],
    ..VALID
};
const DUPLICATE_STATE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[
        EntityLifecycleStateDescriptor {
            id: DRAFT_ID,
            label: "Draft",
        },
        EntityLifecycleStateDescriptor {
            id: DRAFT_ID,
            label: "Draft again",
        },
    ],
    transitions: &[],
    ..VALID
};
const WRONG_INITIAL_LIFECYCLE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    initial: FOREIGN_DRAFT_ID,
    transitions: &[],
    ..VALID
};
const UNDECLARED_INITIAL: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    initial: MISSING_ID,
    transitions: &[],
    ..VALID
};
const WRONG_SOURCE_LIFECYCLE_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: FOREIGN_DRAFT_ID,
        action: ACTION_ID,
        target: ACTIVE_ID,
    }];
const UNDECLARED_SOURCE_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: MISSING_ID,
        action: ACTION_ID,
        target: ACTIVE_ID,
    }];
const WRONG_TARGET_LIFECYCLE_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: ACTION_ID,
        target: FOREIGN_DRAFT_ID,
    }];
const UNDECLARED_TARGET_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: ACTION_ID,
        target: MISSING_ID,
    }];
const WRONG_ACTION_OWNER_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] =
    &[EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: WRONG_OWNER_ACTION_ID,
        target: ACTIVE_ID,
    }];
const DUPLICATE_TRANSITIONS: &[EntityLifecycleTransitionDescriptor] = &[
    EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: ACTION_ID,
        target: ACTIVE_ID,
    },
    EntityLifecycleTransitionDescriptor {
        source: DRAFT_ID,
        action: ACTION_ID,
        target: DRAFT_ID,
    },
];
const WRONG_SOURCE_LIFECYCLE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: WRONG_SOURCE_LIFECYCLE_TRANSITIONS,
    ..VALID
};
const UNDECLARED_SOURCE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: UNDECLARED_SOURCE_TRANSITIONS,
    ..VALID
};
const WRONG_TARGET_LIFECYCLE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: WRONG_TARGET_LIFECYCLE_TRANSITIONS,
    ..VALID
};
const UNDECLARED_TARGET: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: UNDECLARED_TARGET_TRANSITIONS,
    ..VALID
};
const WRONG_ACTION_OWNER: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: WRONG_ACTION_OWNER_TRANSITIONS,
    ..VALID
};
const DUPLICATE_TRANSITION_KEY: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    transitions: DUPLICATE_TRANSITIONS,
    ..VALID
};
const INVALID_LIFECYCLE_LOCAL_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "Workflow",
};
const INVALID_LIFECYCLE_STATE_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: INVALID_LIFECYCLE_LOCAL_ID,
    local: "draft",
};
const INVALID_LIFECYCLE_ID: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    id: INVALID_LIFECYCLE_LOCAL_ID,
    label: "   ",
    states: &[EntityLifecycleStateDescriptor {
        id: INVALID_LIFECYCLE_STATE_ID,
        label: "Draft",
    }],
    initial: INVALID_LIFECYCLE_STATE_ID,
    transitions: &[],
};
const BLANK_LIFECYCLE_LOCAL_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "",
};
const BLANK_LIFECYCLE_STATE_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: BLANK_LIFECYCLE_LOCAL_ID,
    local: "draft",
};
const BLANK_LIFECYCLE_ID: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    id: BLANK_LIFECYCLE_LOCAL_ID,
    label: "Workflow",
    states: &[EntityLifecycleStateDescriptor {
        id: BLANK_LIFECYCLE_STATE_ID,
        label: "Draft",
    }],
    initial: BLANK_LIFECYCLE_STATE_ID,
    transitions: &[],
};
const BLANK_LIFECYCLE_LABEL: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    label: " \t\n ",
    states: &[],
    transitions: &[],
    ..VALID
};
const INVALID_STATE_LOCAL_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "Draft_State",
};
const INVALID_STATE_ID: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[EntityLifecycleStateDescriptor {
        id: INVALID_STATE_LOCAL_ID,
        label: "Draft",
    }],
    initial: INVALID_STATE_LOCAL_ID,
    transitions: &[],
    ..VALID
};
const BLANK_STATE_LOCAL_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "",
};
const BLANK_STATE_ID: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[EntityLifecycleStateDescriptor {
        id: BLANK_STATE_LOCAL_ID,
        label: "Draft",
    }],
    initial: BLANK_STATE_LOCAL_ID,
    transitions: &[],
    ..VALID
};
const BLANK_STATE_LABEL: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    states: &[EntityLifecycleStateDescriptor {
        id: DRAFT_ID,
        label: " \t\n ",
    }],
    initial: DRAFT_ID,
    transitions: &[],
    ..VALID
};

const fn lifecycle(case: u8) -> EntityLifecycleDescriptor {
    match case {
        1 => WRONG_OWNER,
        2 => EMPTY_STATES,
        3 => WRONG_STATE_LIFECYCLE,
        4 => DUPLICATE_STATE,
        5 => WRONG_INITIAL_LIFECYCLE,
        6 => UNDECLARED_INITIAL,
        7 => WRONG_SOURCE_LIFECYCLE,
        8 => UNDECLARED_SOURCE,
        9 => WRONG_TARGET_LIFECYCLE,
        10 => UNDECLARED_TARGET,
        11 => WRONG_ACTION_OWNER,
        12 => DUPLICATE_TRANSITION_KEY,
        13 => INVALID_LIFECYCLE_ID,
        14 => BLANK_LIFECYCLE_ID,
        15 => BLANK_LIFECYCLE_LABEL,
        16 => INVALID_STATE_ID,
        17 => BLANK_STATE_ID,
        18 => BLANK_STATE_LABEL,
        _ => VALID,
    }
}

struct ValidationContext;

impl BoundedContextType for ValidationContext {
    const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
        id: CONTEXT_ID,
        label: "Lifecycle descriptor validation",
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
}

struct ValidationIdentity<const CASE: u8>;

impl<const CASE: u8> DomainIdentityType for ValidationIdentity<CASE> {
    type Owner = ValidationEntity<CASE>;

    const DESCRIPTOR: DomainIdentityDescriptor = DomainIdentityDescriptor {
        id: IDENTITY_ID,
        scalar: ScalarType::U64,
    };
}

fn register<const CASE: u8>() -> Result<(), DomainModelError> {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<ValidationEntity<CASE>>()
}

#[test]
fn accepts_a_well_formed_trusted_descriptor() {
    register::<0>().unwrap();
}

#[test]
fn rejects_a_lifecycle_owned_by_another_entity_before_later_defects() {
    let error = register::<1>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle descriptor owner mismatch")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleDescriptorOwnerMismatch {
            expected: Box::new(ENTITY_ID),
            found: Box::new(FOREIGN_ENTITY_ID),
        }
    );
}

#[test]
fn rejects_an_invalid_lifecycle_local_id_before_its_blank_label() {
    let error = register::<13>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle local id must be nonempty lowercase kebab-case")
    );
    assert_eq!(
        error,
        DomainModelError::InvalidLifecycleLocalId { local: "Workflow" }
    );
}

#[test]
fn rejects_a_blank_lifecycle_local_id() {
    let error = register::<14>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle local id must be nonempty lowercase kebab-case")
    );
    assert_eq!(
        error,
        DomainModelError::InvalidLifecycleLocalId { local: "" }
    );
}

#[test]
fn rejects_a_blank_lifecycle_label_before_state_structure() {
    let error = register::<15>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle label must not be empty")
    );
    assert_eq!(
        error,
        DomainModelError::EmptyLifecycleLabel { label: " \t\n " }
    );
}

#[test]
fn rejects_an_empty_state_collection() {
    let error = register::<2>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("must declare at least one state")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleWithoutStates {
            lifecycle_id: Box::new(LIFECYCLE_ID),
        }
    );
}

#[test]
fn rejects_a_state_owned_by_another_lifecycle() {
    let error = register::<3>().unwrap_err();

    assert!(error.to_string().contains("state ownership mismatch"));
    assert_eq!(
        error,
        DomainModelError::LifecycleStateOwnershipMismatch {
            location: "state",
            expected: Box::new(LIFECYCLE_ID),
            found: Box::new(FOREIGN_LIFECYCLE_ID),
        }
    );
}

#[test]
fn rejects_an_invalid_state_local_id() {
    let error = register::<16>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle state local id must be nonempty lowercase kebab-case")
    );
    assert_eq!(
        error,
        DomainModelError::InvalidLifecycleStateLocalId {
            local: "Draft_State",
        }
    );
}

#[test]
fn rejects_a_blank_state_local_id() {
    let error = register::<17>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle state local id must be nonempty lowercase kebab-case")
    );
    assert_eq!(
        error,
        DomainModelError::InvalidLifecycleStateLocalId { local: "" }
    );
}

#[test]
fn rejects_a_blank_state_label() {
    let error = register::<18>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("entity lifecycle state label must not be empty")
    );
    assert_eq!(
        error,
        DomainModelError::EmptyLifecycleStateLabel { label: " \t\n " }
    );
}

#[test]
fn rejects_duplicate_state_ids_before_initial_validation() {
    let error = register::<4>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("duplicate EntityLifecycleStateId")
    );
    assert_eq!(
        error,
        DomainModelError::DuplicateEntityLifecycleStateId {
            id: Box::new(DRAFT_ID),
        }
    );
}

#[test]
fn rejects_an_initial_state_owned_by_another_lifecycle() {
    let error = register::<5>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("initial state ownership mismatch")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleStateOwnershipMismatch {
            location: "initial state",
            expected: Box::new(LIFECYCLE_ID),
            found: Box::new(FOREIGN_LIFECYCLE_ID),
        }
    );
}

#[test]
fn rejects_an_undeclared_initial_state() {
    let error = register::<6>().unwrap_err();

    assert!(error.to_string().contains("initial state is not declared"));
    assert_eq!(
        error,
        DomainModelError::LifecycleStateNotDeclared {
            location: "initial state",
            id: Box::new(MISSING_ID),
        }
    );
}

#[test]
fn rejects_a_transition_source_owned_by_another_lifecycle() {
    let error = register::<7>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("transition source ownership mismatch")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleStateOwnershipMismatch {
            location: "transition source",
            expected: Box::new(LIFECYCLE_ID),
            found: Box::new(FOREIGN_LIFECYCLE_ID),
        }
    );
}

#[test]
fn rejects_an_undeclared_transition_source() {
    let error = register::<8>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("transition source is not declared")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleStateNotDeclared {
            location: "transition source",
            id: Box::new(MISSING_ID),
        }
    );
}

#[test]
fn rejects_a_transition_target_owned_by_another_lifecycle() {
    let error = register::<9>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("transition target ownership mismatch")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleStateOwnershipMismatch {
            location: "transition target",
            expected: Box::new(LIFECYCLE_ID),
            found: Box::new(FOREIGN_LIFECYCLE_ID),
        }
    );
}

#[test]
fn rejects_an_undeclared_transition_target() {
    let error = register::<10>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("transition target is not declared")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleStateNotDeclared {
            location: "transition target",
            id: Box::new(MISSING_ID),
        }
    );
}

#[test]
fn rejects_a_transition_action_not_owned_by_the_exact_entity() {
    let error = register::<11>().unwrap_err();

    assert!(
        error
            .to_string()
            .contains("transition action owner mismatch")
    );
    assert_eq!(
        error,
        DomainModelError::LifecycleTransitionActionOwnerMismatch {
            expected: Box::new(ActionOwnerId::Entity(ENTITY_ID)),
            found: Box::new(ActionOwnerId::Aggregate(AGGREGATE_ID)),
        }
    );
}

#[test]
fn rejects_duplicate_semantic_transition_keys_even_with_different_targets() {
    let error = register::<12>().unwrap_err();

    assert!(
        error
            .to_string()
            .starts_with("duplicate entity lifecycle transition key")
    );
    assert_eq!(
        error,
        DomainModelError::DuplicateLifecycleTransitionKey {
            source: Box::new(DRAFT_ID),
            action: Box::new(ACTION_ID),
        }
    );
}
