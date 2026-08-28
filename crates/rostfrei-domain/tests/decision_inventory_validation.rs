#![allow(dead_code)]

mod support;

use support::{ExpectedPanicError, panic_message};

use domain::__private::DomainModelBuilder;
use domain::{
    BoundedContext, BoundedContextId, DecisionId, DecisionOwnerId, DomainService, DomainServiceId,
    ValueObject, ValueObjectId, ValueObjectOwnerId, ValueObjectType, domain_decisions,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("decision-inventory");
const SERVICE_ID: DomainServiceId = DomainServiceId {
    context: CONTEXT_ID,
    local: "inventory-service",
};
const INPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-input",
};
const OUTPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "inventory-output",
};
const DECISION_ID: DecisionId = DecisionId {
    owner: DecisionOwnerId::DomainService(SERVICE_ID),
    local: "evaluate",
};

#[derive(BoundedContext)]
#[domain(id = "decision-inventory", label = "Decision inventory")]
struct InventoryContext;

#[derive(ValueObject)]
#[domain(
    id = "inventory-input",
    label = "Inventory input",
    owner = InventoryContext
)]
struct InventoryInput(u64);

#[derive(ValueObject)]
#[domain(
    id = "inventory-output",
    label = "Inventory output",
    owner = InventoryContext
)]
struct InventoryOutput(bool);

#[domain_decisions(domain_service)]
trait InventoryDecisions {
    #[decision(id = "evaluate", label = "Evaluate")]
    fn evaluate(input: InventoryInput) -> InventoryOutput;
}

#[derive(DomainService)]
#[domain(
    id = "inventory-service",
    label = "Inventory service",
    context = InventoryContext,
    decisions = [InventoryDecisions]
)]
struct InventoryService;

impl InventoryDecisions for InventoryService {
    fn evaluate(_input: InventoryInput) -> InventoryOutput {
        InventoryOutput(true)
    }
}

fn violation(missing_id: ValueObjectId, location: &str) -> String {
    format!(
        "Decision reference inventory violation: decision {DECISION_ID:?} references missing {missing_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `value_objects`"
    )
}

fn owner_builder() -> DomainModelBuilder {
    let mut builder = DomainModelBuilder::new();
    builder.add_domain_service_type::<InventoryService>();
    builder
}

#[test]
fn accepts_references_registered_after_the_owner() {
    let mut builder = owner_builder();
    builder.add_value_object(InventoryInput::DESCRIPTOR);
    builder.add_value_object(InventoryOutput::DESCRIPTOR);

    let model = builder.finish();

    assert_eq!(model["decisions"].as_array().unwrap().len(), 1);
}

#[test]
fn reports_a_missing_input_value_object() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = owner_builder();
        builder.add_value_object(InventoryOutput::DESCRIPTOR);
        builder.finish();
    })?;

    assert_eq!(message, violation(INPUT_ID, "input"));
    Ok(())
}

#[test]
fn reports_a_missing_output_value_object() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = owner_builder();
        builder.add_value_object(InventoryInput::DESCRIPTOR);
        builder.finish();
    })?;

    assert_eq!(message, violation(OUTPUT_ID, "output"));
    Ok(())
}

#[test]
fn validates_input_before_output_deterministically() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        owner_builder().finish();
    })?;

    assert_eq!(message, violation(INPUT_ID, "input"));
    Ok(())
}
