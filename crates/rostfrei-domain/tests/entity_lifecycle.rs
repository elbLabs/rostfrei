#![allow(dead_code)]

use std::{collections::HashSet, fmt::Debug, hash::Hash};

use rostfrei_domain::{
    ActionId, ActionOwnerId, ActionReference, Aggregate, AggregateId, BoundedContext,
    BoundedContextId, DomainIdentity, Entity, EntityLifecycleDescriptor, EntityLifecycleId,
    EntityLifecycleStateDescriptor, EntityLifecycleStateId, EntityLifecycleTransitionDescriptor,
    EntityLifecycleType, EntityType,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("planning");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "task-list",
};
const ENTITY_ID: rostfrei_domain::EntityId = rostfrei_domain::EntityId {
    aggregate: AGGREGATE_ID,
    local: "task",
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

#[derive(BoundedContext)]
#[domain(id = "planning", label = "Planning")]
struct Planning;

#[derive(DomainIdentity)]
#[domain(owner = Task)]
struct TaskId(u64);

#[derive(Entity)]
#[domain(id = "task", label = "Task", owner = TaskList)]
struct Task {
    #[domain(identity)]
    id: TaskId,
}

#[derive(Aggregate)]
#[domain(id = "task-list", label = "Task list", context = Planning, root = Task)]
struct TaskList;

enum TaskLifecycleMetadata {
    Draft,
    Active,
    Completed,
}

const ACTIVATE: ActionReference<Task> = ActionReference::__from_local("activate");
const COMPLETE: ActionReference<Task> = ActionReference::__from_local("complete");
const INSPECT: ActionReference<Task> = ActionReference::__from_local("inspect");

impl EntityLifecycleType for TaskLifecycleMetadata {
    type Owner = Task;

    const DESCRIPTOR: EntityLifecycleDescriptor = EntityLifecycleDescriptor {
        id: LIFECYCLE_ID,
        label: "Task workflow",
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
                action: ACTIVATE.id(),
                target: ACTIVE_ID,
            },
            EntityLifecycleTransitionDescriptor {
                source: ACTIVE_ID,
                action: INSPECT.id(),
                target: ACTIVE_ID,
            },
            EntityLifecycleTransitionDescriptor {
                source: ACTIVE_ID,
                action: COMPLETE.id(),
                target: COMPLETED_ID,
            },
        ],
    };
}

fn assert_id_traits<T: Copy + Clone + Debug + Eq + Hash>() {}

fn assert_descriptor_traits<T: Copy + Clone + Debug + Eq>() {}

fn assert_lifecycle_owner<T: EntityLifecycleType<Owner = Task>>() {}

#[test]
fn lifecycle_ids_preserve_value_semantics() {
    assert_id_traits::<EntityLifecycleId>();
    assert_id_traits::<EntityLifecycleStateId>();

    let mut lifecycles = HashSet::new();
    lifecycles.insert(LIFECYCLE_ID);
    lifecycles.insert(EntityLifecycleId {
        owner: ENTITY_ID,
        local: "workflow",
    });
    lifecycles.insert(EntityLifecycleId {
        owner: ENTITY_ID,
        local: "review",
    });

    let mut states = HashSet::new();
    states.insert(DRAFT_ID);
    states.insert(EntityLifecycleStateId {
        lifecycle: LIFECYCLE_ID,
        local: "draft",
    });
    states.insert(ACTIVE_ID);

    assert_eq!(lifecycles.len(), 2);
    assert_eq!(states.len(), 2);
}

#[test]
fn lifecycle_contract_preserves_exact_descriptor_shape_and_order() {
    assert_descriptor_traits::<EntityLifecycleStateDescriptor>();
    assert_descriptor_traits::<EntityLifecycleTransitionDescriptor>();
    assert_descriptor_traits::<EntityLifecycleDescriptor>();
    assert_lifecycle_owner::<TaskLifecycleMetadata>();

    assert_eq!(
        TaskLifecycleMetadata::DESCRIPTOR,
        EntityLifecycleDescriptor {
            id: EntityLifecycleId {
                owner: ENTITY_ID,
                local: "workflow",
            },
            label: "Task workflow",
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
                    action: ActionId {
                        owner: ActionOwnerId::Entity(ENTITY_ID),
                        local: "activate",
                    },
                    target: ACTIVE_ID,
                },
                EntityLifecycleTransitionDescriptor {
                    source: ACTIVE_ID,
                    action: ActionId {
                        owner: ActionOwnerId::Entity(ENTITY_ID),
                        local: "inspect",
                    },
                    target: ACTIVE_ID,
                },
                EntityLifecycleTransitionDescriptor {
                    source: ACTIVE_ID,
                    action: ActionId {
                        owner: ActionOwnerId::Entity(ENTITY_ID),
                        local: "complete",
                    },
                    target: COMPLETED_ID,
                },
            ],
        }
    );

    assert_eq!(
        TaskLifecycleMetadata::DESCRIPTOR
            .states
            .iter()
            .map(|state| state.id.local)
            .collect::<Vec<_>>(),
        ["draft", "active", "completed"]
    );
    assert_eq!(
        TaskLifecycleMetadata::DESCRIPTOR
            .transitions
            .iter()
            .map(|transition| transition.action.local)
            .collect::<Vec<_>>(),
        ["activate", "inspect", "complete"]
    );
}

#[test]
fn transition_descriptors_store_complete_action_ids() {
    let transitions = TaskLifecycleMetadata::DESCRIPTOR.transitions;

    assert_eq!(transitions[0].action, ACTIVATE.id());
    assert_eq!(transitions[1].action, INSPECT.id());
    assert_eq!(transitions[2].action, COMPLETE.id());
    assert!(
        transitions
            .iter()
            .all(|transition| transition.action.owner == ActionOwnerId::Entity(ENTITY_ID))
    );
}

#[test]
fn entities_without_an_attached_lifecycle_default_to_none() {
    assert_eq!(Task::LIFECYCLE, None);
}
