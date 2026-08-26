#![allow(dead_code)]

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

use rostfrei_domain::__private::DomainModelBuilder;
use rostfrei_domain::{
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

fn panic_message(operation: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(operation)).expect_err("operation should panic");
    panic_payload(payload)
}

fn panic_payload(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => panic!("panic payload should be a String or &'static str"),
        },
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
fn reports_a_missing_input_value_object() {
    let message = panic_message(|| {
        let mut builder = owner_builder();
        builder.add_value_object(InventoryOutput::DESCRIPTOR);
        builder.finish();
    });

    assert_eq!(message, violation(INPUT_ID, "input"));
}

#[test]
fn reports_a_missing_output_value_object() {
    let message = panic_message(|| {
        let mut builder = owner_builder();
        builder.add_value_object(InventoryInput::DESCRIPTOR);
        builder.finish();
    });

    assert_eq!(message, violation(OUTPUT_ID, "output"));
}

#[test]
fn validates_input_before_output_deterministically() {
    let message = panic_message(|| {
        owner_builder().finish();
    });

    assert_eq!(message, violation(INPUT_ID, "input"));
}
