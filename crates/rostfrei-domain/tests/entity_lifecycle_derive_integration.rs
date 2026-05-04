#![allow(dead_code)]

use domain::{
    Aggregate, BoundedContext, DomainIdentity, Entity, EntityLifecycle, EntityLifecycleDescriptor,
    EntityLifecycleId, EntityLifecycleStateDescriptor, EntityLifecycleStateId,
    EntityLifecycleTransitionId, EntityLifecycleType, InvalidStateTransition, LifecycleState,
    StateChange, StateTransition, domain_model,
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

#[derive(EntityLifecycle, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "progress", label = "Work item progress")]
#[lifecycle(initial = Pending)]
enum WorkItemLifecycle {
    #[state(id = "pending", label = "Pending")]
    Pending,
    #[state(id = "active", label = "Active")]
    Active,
    #[state(id = "completed", label = "Completed")]
    Completed,
}

#[derive(StateTransition, Clone, Copy, Debug, Eq, PartialEq)]
#[transition(state = WorkItemLifecycle)]
enum WorkItemTransition {
    #[edge(
        id = "start",
        label = "Start",
        from = Pending,
        to = Active
    )]
    Start,
    #[edge(
        id = "complete",
        label = "Complete",
        from = Active,
        to = Completed
    )]
    Complete,
}

#[derive(DomainIdentity)]
struct WorkItemId(u64);

#[derive(Entity)]
#[domain(id = "work-item", label = "Work item")]
struct WorkItem {
    id: WorkItemId,
}

impl domain::EntityDefinition for WorkItem {
    type Owner = WorkQueue;
    type Identity = WorkItemId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
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
        initial: PENDING_ID,
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
    assert_eq!(WorkItemLifecycle::INITIAL, WorkItemLifecycle::Pending);
    assert_eq!(WorkItemLifecycle::Active.state_id(), ACTIVE_ID);

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

#[test]
fn derives_executable_state_transitions() {
    assert_eq!(
        WorkItemLifecycle::Pending.evaluate(&WorkItemTransition::Start),
        Ok(StateChange::new(
            WorkItemLifecycle::Pending,
            WorkItemLifecycle::Active,
        ))
    );
    assert_eq!(
        WorkItemLifecycle::Active.evaluate(&WorkItemTransition::Complete),
        Ok(StateChange::new(
            WorkItemLifecycle::Active,
            WorkItemLifecycle::Completed,
        ))
    );
    assert_eq!(
        WorkItemLifecycle::Completed.evaluate(&WorkItemTransition::Start),
        Err(InvalidStateTransition::new(
            COMPLETED_ID,
            EntityLifecycleTransitionId {
                lifecycle: LIFECYCLE_ID,
                local: "start",
            },
        ))
    );
}

#[test]
fn derives_stable_transition_descriptors() {
    let descriptor = WorkItemTransition::Start.descriptor();

    assert_eq!(descriptor.id.lifecycle, LIFECYCLE_ID);
    assert_eq!(descriptor.id.local, "start");
    assert_eq!(descriptor.label, "Start");
    assert_eq!(descriptor.from, WorkItemLifecycle::Pending);
    assert_eq!(descriptor.to, WorkItemLifecycle::Active);
    assert_eq!(WorkItemTransition::DESCRIPTORS.len(), 2);
}
rostfrei_domain_macros::__install_test_macro_support!();
