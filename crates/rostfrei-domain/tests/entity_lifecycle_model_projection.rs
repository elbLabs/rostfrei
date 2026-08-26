use rostfrei_domain::__private::DomainModelBuilder;
use rostfrei_domain::{
    ActionDescriptor, ActionId, ActionOwnerId, AggregateDescriptor, AggregateId, AggregateType,
    BoundedContextDescriptor, BoundedContextId, BoundedContextType, DomainIdentityDescriptor,
    DomainIdentityId, DomainIdentityType, EntityDescriptor, EntityId, EntityLifecycleDescriptor,
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId,
    EntityLifecycleTransitionDescriptor, EntityType, IdentityDescriptor, ScalarType,
};
use serde_json::json;

const CONTEXT_ID: BoundedContextId = BoundedContextId("lifecycle-projection");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "todo-list",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "todo",
};
const PLAIN_ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "plain",
};
const IDENTITY_ID: DomainIdentityId = DomainIdentityId { owner: ENTITY_ID };
const PLAIN_IDENTITY_ID: DomainIdentityId = DomainIdentityId {
    owner: PLAIN_ENTITY_ID,
};
const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "workflow",
};
const DRAFT_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "draft",
};
const ACTIVE_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "active",
};
const COMPLETED_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "completed",
};
const ACTIVATE_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "activate",
};
const INSPECT_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "inspect",
};
const ARCHIVE_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "archive",
};
const ACTIONS: &[ActionDescriptor] = &[
    ActionDescriptor {
        id: ACTIVATE_ID,
        label: "Activate",
        input: None,
        output: None,
        error: None,
    },
    ActionDescriptor {
        id: INSPECT_ID,
        label: "Inspect",
        input: None,
        output: None,
        error: None,
    },
    ActionDescriptor {
        id: ARCHIVE_ID,
        label: "Archive",
        input: None,
        output: None,
        error: None,
    },
];
const LIFECYCLE: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
    id: LIFECYCLE_ID,
    label: "Todo workflow",
    states: &[
        EntityLifecycleStateDescriptor {
            id: DRAFT_ID,
            label: "Draft",
        },
        EntityLifecycleStateDescriptor {
            id: ACTIVE_ID,
            label: "Active",
        },
        EntityLifecycleStateDescriptor {
            id: COMPLETED_ID,
            label: "Completed",
        },
    ],
    initial: DRAFT_ID,
    transitions: &[
        EntityLifecycleTransitionDescriptor {
            source: DRAFT_ID,
            action: ACTIVATE_ID,
            target: ACTIVE_ID,
        },
        EntityLifecycleTransitionDescriptor {
            source: ACTIVE_ID,
            action: INSPECT_ID,
            target: ACTIVE_ID,
        },
    ],
};

struct ProjectionContext;

impl BoundedContextType for ProjectionContext {
    const DESCRIPTOR: BoundedContextDescriptor = BoundedContextDescriptor {
        id: CONTEXT_ID,
        label: "Lifecycle projection",
    };
}

struct TodoList;

impl AggregateType for TodoList {
    type Context = ProjectionContext;
    type Root = Todo;

    const DESCRIPTOR: AggregateDescriptor = AggregateDescriptor {
        id: AGGREGATE_ID,
        label: "Todo list",
        root: ENTITY_ID,
    };
}

struct Todo;

impl EntityType for Todo {
    type Owner = TodoList;
    type Identity = TodoIdentity;

    const LOCAL_ID: &'static str = "todo";
    const DESCRIPTOR: EntityDescriptor = EntityDescriptor {
        id: ENTITY_ID,
        label: "Todo",
        identity: IdentityDescriptor {
            field: "id",
            identity: IDENTITY_ID,
        },
        fields: &[],
    };
    const LIFECYCLE: Option<EntityLifecycleDescriptor> = Some(LIFECYCLE);
    const ACTION_CONTRACTS: &'static [&'static [ActionDescriptor]] = &[ACTIONS];
}

struct TodoIdentity;

impl DomainIdentityType for TodoIdentity {
    type Owner = Todo;

    const DESCRIPTOR: DomainIdentityDescriptor = DomainIdentityDescriptor {
        id: IDENTITY_ID,
        scalar: ScalarType::U64,
    };
}

struct PlainEntity;

impl EntityType for PlainEntity {
    type Owner = TodoList;
    type Identity = PlainIdentity;

    const LOCAL_ID: &'static str = "plain";
    const DESCRIPTOR: EntityDescriptor = EntityDescriptor {
        id: PLAIN_ENTITY_ID,
        label: "Plain",
        identity: IdentityDescriptor {
            field: "id",
            identity: PLAIN_IDENTITY_ID,
        },
        fields: &[],
    };
}

struct PlainIdentity;

impl DomainIdentityType for PlainIdentity {
    type Owner = PlainEntity;

    const DESCRIPTOR: DomainIdentityDescriptor = DomainIdentityDescriptor {
        id: PLAIN_IDENTITY_ID,
        scalar: ScalarType::U64,
    };
}

#[test]
fn projects_exact_nested_lifecycle_json_in_descriptor_order() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity_type::<Todo>();

    let model = builder.finish();

    assert_eq!(
        model["entities"][0],
        json!({
            "id": {
                "aggregate": {
                    "context": "lifecycle-projection",
                    "local": "todo-list",
                },
                "local": "todo",
            },
            "label": "Todo",
            "identity": {
                "field": "id",
                "id": {
                    "owner": {
                        "aggregate": {
                            "context": "lifecycle-projection",
                            "local": "todo-list",
                        },
                        "local": "todo",
                    },
                },
            },
            "fields": [],
            "lifecycle": {
                "id": "workflow",
                "label": "Todo workflow",
                "states": [
                    { "id": "draft", "label": "Draft" },
                    { "id": "active", "label": "Active" },
                    { "id": "completed", "label": "Completed" },
                ],
                "initial": "draft",
                "transitions": [
                    {
                        "source": "draft",
                        "action": {
                            "owner": {
                                "kind": "entity",
                                "id": {
                                    "aggregate": {
                                        "context": "lifecycle-projection",
                                        "local": "todo-list",
                                    },
                                    "local": "todo",
                                },
                            },
                            "local": "activate",
                        },
                        "target": "active",
                    },
                    {
                        "source": "active",
                        "action": {
                            "owner": {
                                "kind": "entity",
                                "id": {
                                    "aggregate": {
                                        "context": "lifecycle-projection",
                                        "local": "todo-list",
                                    },
                                    "local": "todo",
                                },
                            },
                            "local": "inspect",
                        },
                        "target": "active",
                    },
                ],
            },
        })
    );
    assert!(model.get("lifecycles").is_none());
}

#[test]
fn omits_lifecycle_for_descriptor_registration_and_lifecycle_free_types() {
    let mut builder = DomainModelBuilder::new();
    builder.add_entity(Todo::DESCRIPTOR);
    builder.add_entity_type::<PlainEntity>();

    let model = builder.finish();

    assert_eq!(
        model["entities"],
        json!([
            {
                "id": {
                    "aggregate": {
                        "context": "lifecycle-projection",
                        "local": "todo-list",
                    },
                    "local": "todo",
                },
                "label": "Todo",
                "identity": {
                    "field": "id",
                    "id": {
                        "owner": {
                            "aggregate": {
                                "context": "lifecycle-projection",
                                "local": "todo-list",
                            },
                            "local": "todo",
                        },
                    },
                },
                "fields": [],
            },
            {
                "id": {
                    "aggregate": {
                        "context": "lifecycle-projection",
                        "local": "todo-list",
                    },
                    "local": "plain",
                },
                "label": "Plain",
                "identity": {
                    "field": "id",
                    "id": {
                        "owner": {
                            "aggregate": {
                                "context": "lifecycle-projection",
                                "local": "todo-list",
                            },
                            "local": "plain",
                        },
                    },
                },
                "fields": [],
            },
        ])
    );
}
