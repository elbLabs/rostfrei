#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleDescriptor,
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId, EntityLifecycleType,
    domain_model,
};
const LIFECYCLE_ID: EntityLifecycleId = EntityLifecycleId("progress");
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

#[derive(BoundedContext)]
#[domain(id = "operations", label = "Operations")]
struct Operations;

#[derive(EntityLifecycle)]
#[domain(id = "progress", label = "Work item progress")]
enum WorkItemLifecycle {
    #[state(id = "pending", label = "Pending")]
    Pending,
    #[state(id = "active", label = "Active")]
    Active,
    #[state(id = "completed", label = "Completed")]
    Completed,
}

#[derive(DomainIdentity)]
struct WorkItemId(u64);

#[derive(Entity)]
#[domain(id = "work-item", label = "Work item")]
struct WorkItem {
    #[domain(identity)]
    id: WorkItemId,
}

impl domain::EntityDefinition for WorkItem {
    type Owner = WorkQueue;
    type Identity = WorkItemId;
}

#[derive(Aggregate)]
#[domain(id = "work-queue", label = "Work queue")]
struct WorkQueue;

impl domain::AggregateDefinition for WorkQueue {
    type Context = Operations;
    type Root = WorkItem;
    type Event = domain::NoDomainEvents;
}

#[test]
fn derives_owner_independent_state_metadata_without_model_projection() {
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
    };

    assert_eq!(WorkItemLifecycle::DESCRIPTOR, expected_descriptor);

    let model = domain_model! {
        contexts: [Operations],
        aggregates: [WorkQueue],
        entities: [WorkItem],
        value_objects: [],
        services: [],
        errors: [],
    }
    .expect("entity lifecycle model projection should succeed");

    assert!(model["entities"][0].get("lifecycle").is_none());
    assert!(model["actions"].as_array().unwrap().is_empty());
}
