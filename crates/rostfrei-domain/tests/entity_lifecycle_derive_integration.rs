#![allow(dead_code)]

use domain::{
    ActionId, ActionOwnerId, Aggregate, AggregateId, BoundedContext, BoundedContextId,
    DomainIdentity, Entity, EntityId, EntityLifecycle, EntityLifecycleDescriptor,
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId,
    EntityLifecycleTransitionDescriptor, EntityLifecycleType, EntityType, domain_actions,
    domain_model,
};
use serde_json::json;

const CONTEXT_ID: BoundedContextId = BoundedContextId("operations");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "work-queue",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "work-item",
};
const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId {
    owner: ENTITY_ID,
    local: "progress",
};
const PENDING_ID: EntityLifecycleStateId = EntityLifecycleStateId {
    lifecycle: LIFECYCLE_ID,
    local: "pending",
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
const COMPLETE_ID: ActionId = ActionId {
    owner: ActionOwnerId::Entity(ENTITY_ID),
    local: "complete",
};

#[derive(BoundedContext)]
#[domain(id = "operations", label = "Operations")]
struct Operations;

#[domain_actions(entity)]
trait WorkItemActions {
    #[action(id = "activate", label = "Activate work item")]
    fn activate(&mut self);

    #[action(id = "complete", label = "Complete work item")]
    fn complete(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(
    id = "progress",
    label = "Work item progress",
    owner = WorkItem,
    initial = Pending
)]
enum WorkItemLifecycle {
    #[domain(id = "pending", label = "Pending")]
    #[transition(action = WorkItemActions::ACTIVATE, to = Active)]
    Pending,
    #[domain(id = "active", label = "Active")]
    #[transition(action = WorkItemActions::ACTIVATE, to = Active)]
    #[transition(action = WorkItemActions::COMPLETE, to = Completed)]
    Active,
    #[domain(id = "completed", label = "Completed")]
    Completed,
}

#[derive(DomainIdentity)]
#[domain(owner = WorkItem)]
struct WorkItemId(u64);

#[derive(Entity)]
#[domain(
    id = "work-item",
    label = "Work item",
    owner = WorkQueue,
    actions = [WorkItemActions],
    lifecycle = WorkItemLifecycle
)]
struct WorkItem {
    #[domain(identity)]
    id: WorkItemId,
}

#[derive(Aggregate)]
#[domain(
    id = "work-queue",
    label = "Work queue",
    context = Operations,
    root = WorkItem
)]
struct WorkQueue;

impl WorkItemActions for WorkItem {
    fn activate(&mut self) {}

    fn complete(&mut self) {}
}

#[test]
#[allow(clippy::too_many_lines)]
fn derives_attaches_and_projects_the_entity_lifecycle_contract() {
    let expected_descriptor = EntityLifecycleDescriptor {
        id: LIFECYCLE_ID,
        label: "Work item progress",
        states: &[
            EntityLifecycleStateDescriptor {
                id: PENDING_ID,
                label: "Pending",
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
        initial: PENDING_ID,
        transitions: &[
            EntityLifecycleTransitionDescriptor {
                source: PENDING_ID,
                action: ACTIVATE_ID,
                target: ACTIVE_ID,
            },
            EntityLifecycleTransitionDescriptor {
                source: ACTIVE_ID,
                action: ACTIVATE_ID,
                target: ACTIVE_ID,
            },
            EntityLifecycleTransitionDescriptor {
                source: ACTIVE_ID,
                action: COMPLETE_ID,
                target: COMPLETED_ID,
            },
        ],
    };

    assert_eq!(WorkItemLifecycle::DESCRIPTOR, expected_descriptor);
    assert_eq!(WorkItem::LIFECYCLE, Some(expected_descriptor));

    let model = domain_model! {
        contexts: [Operations],
        aggregates: [WorkQueue],
        entities: [WorkItem],
        identities: [WorkItemId],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    };

    assert_eq!(
        model["entities"][0]["lifecycle"],
        json!({
            "id": "progress",
            "label": "Work item progress",
            "states": [
                { "id": "pending", "label": "Pending" },
                { "id": "active", "label": "Active" },
                { "id": "completed", "label": "Completed" },
            ],
            "initial": "pending",
            "transitions": [
                {
                    "source": "pending",
                    "action": {
                        "owner": {
                            "kind": "entity",
                            "id": {
                                "aggregate": {
                                    "context": "operations",
                                    "local": "work-queue",
                                },
                                "local": "work-item",
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
                                    "context": "operations",
                                    "local": "work-queue",
                                },
                                "local": "work-item",
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
                                    "context": "operations",
                                    "local": "work-queue",
                                },
                                "local": "work-item",
                            },
                        },
                        "local": "complete",
                    },
                    "target": "completed",
                },
            ],
        })
    );

    let transitions = model["entities"][0]["lifecycle"]["transitions"]
        .as_array()
        .unwrap();
    let actions = model["actions"].as_array().unwrap();

    assert_eq!(actions.len(), 2);
    assert_eq!(transitions[0]["action"], actions[0]["id"]);
    assert_eq!(transitions[1]["action"], actions[0]["id"]);
    assert_eq!(transitions[2]["action"], actions[1]["id"]);
    assert!(model.get("lifecycles").is_none());
}
