#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
use domain::DecisionOutcome;
use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DecisionId, DecisionOwnerId,
    DomainIdentity, DomainModelError, Entity, ValueObject, ValueObjectId, ValueObjectOwnerId,
    ValueObjectType, domain_decisions,
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
#[domain(
    id = "inventory-aggregate",
    label = "Inventory aggregate",
    context = InventoryContext,
    root = InventoryRoot,
    decisions = [InventoryDecisions]
)]
struct InventoryAggregate;

#[derive(Entity)]
#[domain(id = "inventory-root", label = "Inventory root", owner = InventoryAggregate)]
struct InventoryRoot {
    #[domain(identity)]
    id: InventoryIdentity,
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

fn owner_builder() -> Result<DomainModelBuilder, DomainModelError> {
    let mut builder = DomainModelBuilder::new();
    builder.add_aggregate_type::<InventoryAggregate>()?;
    Ok(builder)
}

#[test]
fn accepts_references_registered_after_the_owner() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryAccepted::DESCRIPTOR);
    builder.add_value_object(InventoryRejected::DESCRIPTOR);

    let model = builder.finish().unwrap();

    assert_eq!(model["decisions"].as_array().unwrap().len(), 1);
}

#[test]
fn reports_a_missing_input_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryAccepted::DESCRIPTOR);
    builder.add_value_object(InventoryRejected::DESCRIPTOR);

    let error = builder.finish().unwrap_err();

    assert_eq!(
        error.to_string(),
        violation(INPUT_ID, "parameters[0].input")
    );
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(INPUT_ID),
            location: "parameters[0].input".to_owned(),
        }
    );
}

#[test]
fn reports_a_missing_tuple_outcome_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryRejected::DESCRIPTOR);

    let error = builder.finish().unwrap_err();
    let location = "outcomes[0].shape.fields[0]";

    assert_eq!(error.to_string(), violation(ACCEPTED_ID, location));
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(ACCEPTED_ID),
            location: location.to_owned(),
        }
    );
}

#[test]
fn reports_a_missing_struct_outcome_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryAccepted::DESCRIPTOR);

    let error = builder.finish().unwrap_err();
    let location = "outcomes[1].shape.fields[0].value";

    assert_eq!(error.to_string(), violation(REJECTED_ID, location));
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(REJECTED_ID),
            location: location.to_owned(),
        }
    );
}

#[test]
fn validates_parameters_then_outcomes_in_source_and_field_order() {
    let error = owner_builder().unwrap().finish().unwrap_err();

    assert_eq!(
        error.to_string(),
        violation(INPUT_ID, "parameters[0].input")
    );
}
