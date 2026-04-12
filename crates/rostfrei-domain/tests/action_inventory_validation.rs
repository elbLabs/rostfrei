#![allow(dead_code)]

use std::any::Any;
use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainCommand, DomainCommandId,
    DomainCommandOwnerId, DomainCommandType, DomainError, DomainErrorId, DomainErrorOwnerId,
    DomainErrorType, DomainEvent, DomainEventId, DomainEventType, DomainIdentity, DomainService,
    DomainServiceId, Entity, EntityId, ValueObject, ValueObjectId, ValueObjectOwnerId,
    ValueObjectType, domain_actions,
};

const CONTEXT_ID: BoundedContextId = BoundedContextId("action-inventory");
const AGGREGATE_ID: AggregateId = AggregateId {
    context: CONTEXT_ID,
    local: "inventory-aggregate",
};
const ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "inventory-entity",
};
const VALUE_OBJECT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Aggregate(AGGREGATE_ID),
    local: "inventory-value",
};
const INPUT_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Entity(ENTITY_ID),
    local: "input-value",
};
const OUTPUT_VALUE_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Entity(ENTITY_ID),
    local: "output-value",
};
const SERVICE_ID: DomainServiceId = DomainServiceId {
    context: CONTEXT_ID,
    local: "inventory-service",
};
const EXTENSION_SERVICE_ID: DomainServiceId = DomainServiceId {
    context: CONTEXT_ID,
    local: "extension-service",
};
const ORDERED_ENTITY_ID: EntityId = EntityId {
    aggregate: AGGREGATE_ID,
    local: "ordered-entity",
};
const ORDERED_INPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::Entity(ORDERED_ENTITY_ID),
    local: "ordered-input",
};
const AGGREGATE_COMMAND_ID: DomainCommandId = DomainCommandId {
    owner: DomainCommandOwnerId::Aggregate(AGGREGATE_ID),
    local: "aggregate-command",
};
const SERVICE_COMMAND_ID: DomainCommandId = DomainCommandId {
    owner: DomainCommandOwnerId::DomainService(SERVICE_ID),
    local: "service-command",
};
const MISSING_COMMAND_ID: DomainCommandId = DomainCommandId {
    owner: DomainCommandOwnerId::DomainService(EXTENSION_SERVICE_ID),
    local: "missing-command",
};
const EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AGGREGATE_ID,
    local: "inventory-event",
};
const MISSING_EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AGGREGATE_ID,
    local: "missing-event",
};
const AGGREGATE_ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::Aggregate(AGGREGATE_ID),
    local: "aggregate-error",
};
const SERVICE_ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::DomainService(SERVICE_ID),
    local: "service-error",
};
const ENTITY_ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::Entity(ENTITY_ID),
    local: "entity-error",
};
const VALUE_ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::ValueObject(VALUE_OBJECT_ID),
    local: "value-error",
};
const MISSING_ERROR_ID: DomainErrorId = DomainErrorId {
    owner: DomainErrorOwnerId::DomainService(EXTENSION_SERVICE_ID),
    local: "missing-error",
};

#[derive(BoundedContext)]
#[domain(id = "action-inventory", label = "Action inventory")]
pub struct InventoryContext;

#[derive(DomainIdentity)]
#[domain(owner = InventoryEntity)]
pub struct InventoryEntityIdentity(u64);

#[domain_actions(aggregate)]
pub trait AggregateActions {
    #[action(id = "aggregate-action", label = "Aggregate action")]
    fn execute(
        root: &mut InventoryEntity,
        input: AggregateCommand,
    ) -> Result<InventoryEvent, AggregateError>;
}

#[derive(Aggregate)]
#[domain(
    id = "inventory-aggregate",
    label = "Inventory aggregate",
    context = InventoryContext,
    root = InventoryEntity,
    actions = [AggregateActions]
)]
pub struct InventoryAggregate;

#[domain_actions(entity)]
trait EntityActions {
    #[action(id = "entity-action", label = "Entity action")]
    fn transform(&self, input: InputValue) -> Result<OutputValue, EntityError>;
}

#[derive(Entity)]
#[domain(
    id = "inventory-entity",
    label = "Inventory entity",
    owner = InventoryAggregate,
    actions = [EntityActions]
)]
pub struct InventoryEntity {
    #[domain(identity)]
    id: InventoryEntityIdentity,
}

impl AggregateActions for InventoryAggregate {
    fn execute(
        _root: &mut InventoryEntity,
        _input: AggregateCommand,
    ) -> Result<InventoryEvent, AggregateError> {
        Ok(InventoryEvent)
    }
}

impl EntityActions for InventoryEntity {
    fn transform(&self, _input: InputValue) -> Result<OutputValue, EntityError> {
        Ok(OutputValue(0))
    }
}

#[derive(DomainCommand)]
#[domain(
    id = "aggregate-command",
    label = "Aggregate command",
    owner = InventoryAggregate
)]
pub struct AggregateCommand;

#[derive(DomainEvent)]
#[domain(
    id = "inventory-event",
    label = "Inventory event",
    owner = InventoryAggregate
)]
pub struct InventoryEvent;

#[derive(DomainError)]
#[domain(
    id = "aggregate-error",
    label = "Aggregate error",
    owner = InventoryAggregate,
    code = "AGGREGATE_ERROR",
    message = "Aggregate error."
)]
pub struct AggregateError;

#[derive(ValueObject)]
#[domain(
    id = "input-value",
    label = "Input value",
    owner = InventoryEntity
)]
struct InputValue(u64);

#[derive(ValueObject)]
#[domain(
    id = "output-value",
    label = "Output value",
    owner = InventoryEntity
)]
struct OutputValue(u64);

#[derive(DomainError)]
#[domain(
    id = "entity-error",
    label = "Entity error",
    owner = InventoryEntity,
    code = "ENTITY_ERROR",
    message = "Entity error."
)]
struct EntityError;

#[domain_actions(domain_service)]
pub trait ServiceActions {
    #[action(id = "service-action", label = "Service action")]
    fn execute(input: ServiceCommand) -> Result<InventoryEvent, ServiceError>;
}

#[derive(DomainService)]
#[domain(
    id = "inventory-service",
    label = "Inventory service",
    context = InventoryContext,
    actions = [ServiceActions]
)]
pub struct InventoryService;

impl ServiceActions for InventoryService {
    fn execute(_input: ServiceCommand) -> Result<InventoryEvent, ServiceError> {
        Ok(InventoryEvent)
    }
}

#[derive(DomainCommand)]
#[domain(
    id = "service-command",
    label = "Service command",
    owner = InventoryService
)]
pub struct ServiceCommand;

#[derive(DomainError)]
#[domain(
    id = "service-error",
    label = "Service error",
    owner = InventoryService,
    code = "SERVICE_ERROR",
    message = "Service error."
)]
pub struct ServiceError;

#[domain_actions(value_object)]
trait ValueActions {
    #[action(id = "value-action", label = "Value action")]
    fn transform(self, input: InputValue) -> Result<Self, ValueError>;
}

#[derive(ValueObject)]
#[domain(
    id = "inventory-value",
    label = "Inventory value",
    owner = InventoryAggregate,
    actions = [ValueActions]
)]
struct InventoryValue(u64);

impl ValueActions for InventoryValue {
    fn transform(self, _input: InputValue) -> Result<Self, ValueError> {
        Ok(self)
    }
}

#[derive(DomainError)]
#[domain(
    id = "value-error",
    label = "Value error",
    owner = InventoryValue,
    code = "VALUE_ERROR",
    message = "Value error."
)]
struct ValueError;

#[derive(DomainService)]
#[domain(
    id = "extension-service",
    label = "Extension service",
    context = InventoryContext
)]
struct ExtensionService;

#[derive(DomainIdentity)]
#[domain(owner = OrderedEntity)]
struct OrderedEntityIdentity(u64);

#[domain_actions(entity)]
trait OrderedAttachedActions {
    #[action(id = "attached-missing", label = "Attached missing")]
    fn attached(&self, input: OrderedInput);
}

#[derive(Entity)]
#[domain(
    id = "ordered-entity",
    label = "Ordered entity",
    owner = InventoryAggregate,
    actions = [OrderedAttachedActions]
)]
struct OrderedEntity {
    #[domain(identity)]
    id: OrderedEntityIdentity,
}

impl OrderedAttachedActions for OrderedEntity {
    fn attached(&self, _input: OrderedInput) {}
}

#[derive(ValueObject)]
#[domain(
    id = "ordered-input",
    label = "Ordered input",
    owner = OrderedEntity
)]
struct OrderedInput(u64);

const fn extension_action(
    local: &'static str,
    input: Option<ActionInputDescriptor>,
    output: Option<ActionOutputDescriptor>,
    error: Option<DomainErrorId>,
) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::DomainService(EXTENSION_SERVICE_ID),
            local,
        },
        label: local,
        input,
        output,
        error,
    }
}

struct DeterministicExtension;

impl ActionGroupType for DeterministicExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[extension_action(
        "deterministic",
        Some(ActionInputDescriptor::DomainCommand(MISSING_COMMAND_ID)),
        Some(ActionOutputDescriptor::DomainEvent(MISSING_EVENT_ID)),
        Some(MISSING_ERROR_ID),
    )];
}

struct NestedExtension;

impl ActionGroupType for NestedExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[extension_action(
        "nested",
        None,
        Some(ActionOutputDescriptor::Optional(
            &ActionOutputDescriptor::List(&ActionOutputDescriptor::Optional(
                &ActionOutputDescriptor::List(&ActionOutputDescriptor::DomainEvent(
                    MISSING_EVENT_ID,
                )),
            )),
        )),
        None,
    )];
}

struct DanglingExtension;

impl ActionGroupType for DanglingExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[extension_action(
        "dangling-extension",
        None,
        Some(ActionOutputDescriptor::DomainEvent(MISSING_EVENT_ID)),
        None,
    )];
}

struct OrderedExtension;

impl ActionGroupType for OrderedExtension {
    type Owner = OrderedEntity;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Entity(ORDERED_ENTITY_ID),
            local: "extension-missing",
        },
        label: "Extension missing",
        input: None,
        output: Some(ActionOutputDescriptor::ValueObject(OUTPUT_VALUE_ID)),
        error: None,
    }];
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

fn violation(
    action_id: ActionId,
    missing_id: impl Debug,
    location: &str,
    inventory_key: &str,
) -> String {
    format!(
        "Action reference inventory violation: action {action_id:?} references missing {missing_id:?} at descriptor location `{location}`; add it to domain_model! inventory key `{inventory_key}`"
    )
}

fn extension_action_id(local: &'static str) -> ActionId {
    ActionId {
        owner: ActionOwnerId::DomainService(EXTENSION_SERVICE_ID),
        local,
    }
}

#[test]
fn accepts_references_added_after_all_owner_actions_are_registered() {
    let mut builder = DomainModelBuilder::new();
    builder.add_aggregate_type::<InventoryAggregate>();
    builder.add_domain_service_type::<InventoryService>();
    builder.add_entity_type::<InventoryEntity>();
    builder.add_value_object_type::<InventoryValue>();

    builder.add_domain_command(AggregateCommand::DESCRIPTOR);
    builder.add_domain_command(ServiceCommand::DESCRIPTOR);
    builder.add_domain_event(InventoryEvent::DESCRIPTOR);
    builder.add_domain_error(AggregateError::DESCRIPTOR);
    builder.add_domain_error(ServiceError::DESCRIPTOR);
    builder.add_domain_error(EntityError::DESCRIPTOR);
    builder.add_domain_error(ValueError::DESCRIPTOR);
    builder.add_value_object(InputValue::DESCRIPTOR);
    builder.add_value_object(OutputValue::DESCRIPTOR);
    builder.add_domain_identity_type::<InventoryEntityIdentity>();

    let model = builder.finish();

    assert_eq!(model["actions"].as_array().unwrap().len(), 4);
}

#[test]
fn aggregate_reports_missing_command_input() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_aggregate_type::<InventoryAggregate>();
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
                local: "aggregate-action",
            },
            AGGREGATE_COMMAND_ID,
            "input",
            "commands",
        )
    );
}

#[test]
fn domain_service_reports_missing_event_output() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<InventoryService>();
        builder.add_domain_command(ServiceCommand::DESCRIPTOR);
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::DomainService(SERVICE_ID),
                local: "service-action",
            },
            EVENT_ID,
            "output",
            "events",
        )
    );
}

#[test]
fn entity_reports_missing_value_object_input() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<InventoryEntity>();
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Entity(ENTITY_ID),
                local: "entity-action",
            },
            INPUT_VALUE_ID,
            "input",
            "value_objects",
        )
    );
}

#[test]
fn entity_reports_missing_value_object_output() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<InventoryEntity>();
        builder.add_value_object(InputValue::DESCRIPTOR);
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Entity(ENTITY_ID),
                local: "entity-action",
            },
            OUTPUT_VALUE_ID,
            "output",
            "value_objects",
        )
    );
}

#[test]
fn value_object_reports_missing_error() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_value_object_type::<InventoryValue>();
        builder.add_value_object(InputValue::DESCRIPTOR);
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::ValueObject(VALUE_OBJECT_ID),
                local: "value-action",
            },
            VALUE_ERROR_ID,
            "error",
            "errors",
        )
    );
}

#[test]
fn traverses_deeply_nested_optional_list_outputs() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<NestedExtension>();
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            extension_action_id("nested"),
            MISSING_EVENT_ID,
            "output.optional.value.list.element.optional.value.list.element",
            "events",
        )
    );
}

#[test]
fn reports_descriptor_failures_in_input_output_error_order() {
    let input_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.finish();
    });
    let output_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.add_domain_command(domain::DomainCommandDescriptor {
            id: MISSING_COMMAND_ID,
            label: "Missing command",
            fields: &[],
        });
        builder.finish();
    });
    let error_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.add_domain_command(domain::DomainCommandDescriptor {
            id: MISSING_COMMAND_ID,
            label: "Missing command",
            fields: &[],
        });
        builder.add_domain_event(domain::DomainEventDescriptor {
            id: MISSING_EVENT_ID,
            label: "Missing event",
            fields: &[],
        });
        builder.finish();
    });

    assert_eq!(
        input_message,
        violation(
            extension_action_id("deterministic"),
            MISSING_COMMAND_ID,
            "input",
            "commands",
        )
    );
    assert_eq!(
        output_message,
        violation(
            extension_action_id("deterministic"),
            MISSING_EVENT_ID,
            "output",
            "events",
        )
    );
    assert_eq!(
        error_message,
        violation(
            extension_action_id("deterministic"),
            MISSING_ERROR_ID,
            "error",
            "errors",
        )
    );
}

#[test]
fn validates_attached_actions_before_extensions() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<OrderedEntity>();
        builder.add_action_extension::<OrderedExtension>();
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Entity(ORDERED_ENTITY_ID),
                local: "attached-missing",
            },
            ORDERED_INPUT_ID,
            "input",
            "value_objects",
        )
    );
}

#[test]
fn action_group_extension_reports_dangling_reference() {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DanglingExtension>();
        builder.finish();
    });

    assert_eq!(
        message,
        violation(
            extension_action_id("dangling-extension"),
            MISSING_EVENT_ID,
            "output",
            "events",
        )
    );
}
