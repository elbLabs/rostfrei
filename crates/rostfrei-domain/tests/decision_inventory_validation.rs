#![allow(dead_code)]

use domain::__private::DomainModelBuilder;
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
const OUTPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-output",
};
const ERROR_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-error",
};
const DECISION_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::Aggregate(AGGREGATE_ID),
    local: "evaluate",
};

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
    decisions
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
#[domain(id = "inventory-output", label = "Inventory output", owner = InventoryContext)]
struct InventoryOutput(bool);

#[derive(ValueObject)]
#[domain(id = "inventory-error", label = "Inventory error", owner = InventoryContext)]
struct InventoryError;

#[domain_decisions(aggregate)]
impl InventoryAggregate {
    #[decision(id = "evaluate", label = "Evaluate")]
    const fn evaluate(input: InventoryInput) -> Result<InventoryOutput, InventoryError> {
        if input.0 > 0 {
            Ok(InventoryOutput(true))
        } else {
            Err(InventoryError)
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
    builder.add_value_object(InventoryOutput::DESCRIPTOR);
    builder.add_value_object(InventoryError::DESCRIPTOR);

    let model = builder.finish().unwrap();

    assert_eq!(model["decisions"].as_array().unwrap().len(), 1);
}

#[test]
fn reports_a_missing_input_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryOutput::DESCRIPTOR);
    builder.add_value_object(InventoryError::DESCRIPTOR);

    let error = builder.finish().unwrap_err();

    assert_eq!(error.to_string(), violation(INPUT_ID, "input"));
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(INPUT_ID),
            location: "input",
        }
    );
}

#[test]
fn reports_a_missing_output_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryError::DESCRIPTOR);

    let error = builder.finish().unwrap_err();

    assert_eq!(error.to_string(), violation(OUTPUT_ID, "output"));
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(OUTPUT_ID),
            location: "output",
        }
    );
}

#[test]
fn reports_a_missing_error_value_object() {
    let mut builder = owner_builder().unwrap();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryOutput::DESCRIPTOR);

    let error = builder.finish().unwrap_err();

    assert_eq!(error.to_string(), violation(ERROR_ID, "error"));
    assert_eq!(
        error,
        DomainModelError::DecisionReferenceInventoryViolation {
            decision_id: Box::new(DECISION_ID),
            value_object_id: Box::new(ERROR_ID),
            location: "error",
        }
    );
}

#[test]
fn validates_parameter_output_and_error_references_in_order() {
    let error = owner_builder().unwrap().finish().unwrap_err();

    assert_eq!(error.to_string(), violation(INPUT_ID, "input"));
}
