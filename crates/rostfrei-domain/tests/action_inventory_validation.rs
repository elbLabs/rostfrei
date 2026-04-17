#![allow(dead_code)]

use std::fmt::Debug;

mod support;

use support::{ExpectedPanicError, panic_message};

use domain::__private::DomainModelBuilder;
use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainCommand, DomainCommandType,
    DomainError, DomainErrorId, DomainErrorOwnerId, DomainErrorType, DomainEvent, DomainEventId,
    DomainIdentity, DomainIdentityId, DomainService, DomainServiceId, Entity, EntityId,
    ValueObject, ValueObjectId, ValueObjectOwnerId, ValueObjectType, domain_actions,
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
const INVENTORY_IDENTITY_ID: DomainIdentityId = DomainIdentityId { owner: ENTITY_ID };
const SERVICE_ID: DomainServiceId = DomainServiceId {
    context: CONTEXT_ID,
    local: "inventory-service",
};
const SERVICE_ACTION_INPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "service-action-input",
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
const EXTENSION_INPUT_ID: ValueObjectId = ValueObjectId {
    owner: ValueObjectOwnerId::BoundedContext(CONTEXT_ID),
    local: "extension-input",
};
const EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AGGREGATE_ID,
    local: "inventory-event",
};
const MISSING_EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AGGREGATE_ID,
    local: "missing-event",
};
const FOREIGN_EVENT_ID: DomainEventId = DomainEventId {
    aggregate: AggregateId {
        context: CONTEXT_ID,
        local: "foreign-aggregate",
    },
    local: "foreign-event",
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
        input: InventoryEntityIdentity,
    ) -> Result<InventoryEvent, AggregateError>;
}

#[derive(Aggregate)]
#[domain(
    id = "inventory-aggregate",
    label = "Inventory aggregate",
    context = InventoryContext,
    root = InventoryEntity,
    actions = [AggregateActions],
    events = [InventoryEvent]
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
        _input: InventoryEntityIdentity,
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
#[domain(id = "inventory-event", label = "Inventory event")]
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
    fn execute(input: ServiceActionInput) -> Result<InventoryEvent, ServiceError>;
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
    fn execute(_input: ServiceActionInput) -> Result<InventoryEvent, ServiceError> {
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

#[derive(ValueObject)]
#[domain(
    id = "service-action-input",
    label = "Service action input",
    owner = InventoryContext
)]
pub struct ServiceActionInput;

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

#[derive(ValueObject)]
#[domain(
    id = "extension-input",
    label = "Extension input",
    owner = InventoryContext
)]
struct ExtensionInput;

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
        raises: &[],
        error,
    }
}

struct DeterministicExtension;

impl ActionGroupType for DeterministicExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[extension_action(
        "deterministic",
        Some(ActionInputDescriptor::ValueObject(EXTENSION_INPUT_ID)),
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

struct RaisedEventExtension;

impl ActionGroupType for RaisedEventExtension {
    type Owner = InventoryAggregate;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
            local: "raised-event",
        },
        label: "Raised event",
        input: None,
        output: None,
        raises: &[MISSING_EVENT_ID],
        error: None,
    }];
}

struct NonAggregateRaisedEventExtension;

impl ActionGroupType for NonAggregateRaisedEventExtension {
    type Owner = ExtensionService;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::DomainService(EXTENSION_SERVICE_ID),
            local: "invalid-raised-event-owner",
        },
        label: "Invalid raised event owner",
        input: None,
        output: None,
        raises: &[MISSING_EVENT_ID],
        error: None,
    }];
}

struct ForeignRaisedEventExtension;

impl ActionGroupType for ForeignRaisedEventExtension {
    type Owner = InventoryAggregate;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
            local: "foreign-raised-event",
        },
        label: "Foreign raised event",
        input: None,
        output: None,
        raises: &[FOREIGN_EVENT_ID],
        error: None,
    }];
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
        raises: &[],
        error: None,
    }];
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
    builder.add_domain_error(AggregateError::DESCRIPTOR);
    builder.add_domain_error(ServiceError::DESCRIPTOR);
    builder.add_domain_error(EntityError::DESCRIPTOR);
    builder.add_domain_error(ValueError::DESCRIPTOR);
    builder.add_value_object(InputValue::DESCRIPTOR);
    builder.add_value_object(OutputValue::DESCRIPTOR);
    builder.add_value_object(ServiceActionInput::DESCRIPTOR);
    builder.add_domain_identity_type::<InventoryEntityIdentity>();

    let model = builder.finish();

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(actions.len(), 4);
    assert_eq!(actions[0]["input"]["kind"], "domainIdentity");
    assert_eq!(
        actions[0]["input"]["id"]["owner"]["local"],
        "inventory-entity"
    );
}

#[test]
fn aggregate_reports_missing_domain_identity_input() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_aggregate_type::<InventoryAggregate>();
        builder.finish();
    })?;

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
                local: "aggregate-action",
            },
            INVENTORY_IDENTITY_ID,
            "input",
            "identities",
        )
    );
    Ok(())
}

#[test]
fn domain_service_reports_missing_event_output() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<InventoryService>();
        builder.add_value_object(ServiceActionInput::DESCRIPTOR);
        builder.finish();
    })?;

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
    Ok(())
}

#[test]
fn entity_reports_missing_value_object_input() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<InventoryEntity>();
        builder.finish();
    })?;

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
    Ok(())
}

#[test]
fn entity_reports_missing_value_object_output() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<InventoryEntity>();
        builder.add_value_object(InputValue::DESCRIPTOR);
        builder.finish();
    })?;

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
    Ok(())
}

#[test]
fn value_object_reports_missing_error() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_value_object_type::<InventoryValue>();
        builder.add_value_object(InputValue::DESCRIPTOR);
        builder.finish();
    })?;

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
    Ok(())
}

#[test]
fn traverses_deeply_nested_optional_list_outputs() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<NestedExtension>();
        builder.finish();
    })?;

    assert_eq!(
        message,
        violation(
            extension_action_id("nested"),
            MISSING_EVENT_ID,
            "output.optional.value.list.element.optional.value.list.element",
            "events",
        )
    );
    Ok(())
}

#[test]
fn reports_missing_raised_event() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_aggregate_type::<InventoryAggregate>();
        builder.add_action_extension::<RaisedEventExtension>();
        builder.add_domain_identity_type::<InventoryEntityIdentity>();
        builder.add_domain_error(AggregateError::DESCRIPTOR);
        builder.finish();
    })?;

    assert_eq!(
        message,
        violation(
            ActionId {
                owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
                local: "raised-event",
            },
            MISSING_EVENT_ID,
            "raises[0]",
            "events",
        )
    );
    Ok(())
}

#[test]
fn rejects_raised_events_on_non_aggregate_actions() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<NonAggregateRaisedEventExtension>();
        builder.finish();
    })?;

    assert_eq!(
        message,
        format!(
            "Action raised-event owner violation: action {:?} is not owned by an Aggregate",
            extension_action_id("invalid-raised-event-owner")
        )
    );
    Ok(())
}

#[test]
fn rejects_another_aggregates_raised_event() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_aggregate_type::<InventoryAggregate>();
        builder.add_action_extension::<ForeignRaisedEventExtension>();
        builder.add_domain_identity_type::<InventoryEntityIdentity>();
        builder.add_domain_error(AggregateError::DESCRIPTOR);
        builder.finish();
    })?;
    let action_id = ActionId {
        owner: ActionOwnerId::Aggregate(AGGREGATE_ID),
        local: "foreign-raised-event",
    };

    assert_eq!(
        message,
        format!(
            "Action raised-event owner violation: action {action_id:?} declares event {FOREIGN_EVENT_ID:?} owned by another Aggregate"
        )
    );
    Ok(())
}

#[test]
fn reports_descriptor_failures_in_input_output_error_order() -> Result<(), ExpectedPanicError> {
    let input_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.finish();
    })?;
    let output_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.add_value_object(ExtensionInput::DESCRIPTOR);
        builder.finish();
    })?;
    let error_message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DeterministicExtension>();
        builder.add_value_object(ExtensionInput::DESCRIPTOR);
        builder.add_domain_event(domain::DomainEventDescriptor {
            id: MISSING_EVENT_ID,
            label: "Missing event",
            schema_version: 1,
            fields: &[],
        });
        builder.finish();
    })?;

    assert_eq!(
        input_message,
        violation(
            extension_action_id("deterministic"),
            EXTENSION_INPUT_ID,
            "input",
            "value_objects",
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
    Ok(())
}

#[test]
fn validates_attached_actions_before_extensions() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_entity_type::<OrderedEntity>();
        builder.add_action_extension::<OrderedExtension>();
        builder.finish();
    })?;

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
    Ok(())
}

#[test]
fn action_group_extension_reports_dangling_reference() -> Result<(), ExpectedPanicError> {
    let message = panic_message(|| {
        let mut builder = DomainModelBuilder::new();
        builder.add_domain_service_type::<ExtensionService>();
        builder.add_action_extension::<DanglingExtension>();
        builder.finish();
    })?;

    assert_eq!(
        message,
        violation(
            extension_action_id("dangling-extension"),
            MISSING_EVENT_ID,
            "output",
            "events",
        )
    );
    Ok(())
}
