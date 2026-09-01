#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DecisionId, DecisionOwnerId,
    DomainIdentity, Entity, ValueObject, ValueObjectId, ValueObjectOwnerId, domain_decisions,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("decision-inventory");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "inventory-aggregate",
};
const INPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-input",
};
const ACCEPTED_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-accepted",
};
const REJECTED_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-rejected",
};
const DECISION_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::Aggregate(AGGREGATE_ID),
    local: "evaluate",
};

struct InventoryDecisions;

#[derive(BoundedContext)]
#[domain(id = "decision-inventory", label = "Decision inventory")]
struct InventoryContext;

#[derive(DomainIdentity)]
#[domain(owner = InventoryRoot)]
struct InventoryIdentity(u64);

#[derive(Aggregate)]
#[domain(id = "inventory-aggregate", label = "Inventory aggregate")]
struct InventoryAggregate;

impl domain::AggregateDefinition for InventoryAggregate {
    type Context = InventoryContext;
    type Root = InventoryRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(Entity)]
#[domain(id = "inventory-root", label = "Inventory root")]
struct InventoryRoot {
    #[domain(identity)]
    id: InventoryIdentity,
}

impl domain::EntityDefinition for InventoryRoot {
    type Owner = InventoryAggregate;
    type Identity = InventoryIdentity;
}

#[derive(ValueObject, Clone, Copy)]
#[domain(id = "inventory-input", label = "Inventory input", owner = InventoryContext)]
struct InventoryInput(u64);

#[derive(ValueObject)]
#[domain(id = "inventory-accepted", label = "Inventory accepted", owner = InventoryContext)]
struct InventoryAccepted(bool);

#[derive(ValueObject)]
#[domain(id = "inventory-rejected", label = "Inventory rejected", owner = InventoryContext)]
struct InventoryRejected;

#[derive(DecisionOutcome)]
enum InventoryOutcome {
    #[outcome(id = "accepted", label = "Accepted")]
    Accepted(InventoryAccepted, bool),
    #[outcome(id = "rejected", label = "Rejected")]
    Rejected {
        reason: InventoryRejected,
        retryable: bool,
    },
}

#[domain_decisions(aggregate, group = InventoryDecisions)]
impl InventoryAggregate {
    #[decision(id = "evaluate", label = "Evaluate")]
    const fn evaluate(input: InventoryInput) -> InventoryOutcome {
        if input.0 > 0 {
            InventoryOutcome::Accepted(InventoryAccepted(true), true)
        } else {
            InventoryOutcome::Rejected {
                reason: InventoryRejected,
                retryable: false,
            }
        }
    }
}

fn violation(missing_id: ValueObjectId, location: &str) -> String {
    format!(
        "Decision reference inventory violation: decision {DECISION_ID:?} references missing {missing_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `value_objects`"
    )
}

#[test]
fn aggregate_decisions_are_not_implicitly_registered() {
    let mut builder = DomainModelBuilder::new();
    builder.add_aggregate_type::<InventoryAggregate>().unwrap();
    let model = builder.finish().unwrap();
    assert!(model["decisions"].as_array().unwrap().is_empty());
}
