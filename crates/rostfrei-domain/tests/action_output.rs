#![allow(dead_code)]

use domain::{
    ActionOutputDescriptor, Aggregate, BoundedContext, DomainError, DomainErrorType, DomainEvent,
    DomainEventType, DomainIdentity, DomainService, Entity, ScalarType, ValueObject,
    ValueObjectType, domain_actions, domain_model,
};

#[derive(BoundedContext)]
#[domain(id = "operations", label = "Operations")]
pub struct Operations;

#[derive(DomainIdentity)]
#[domain(owner = MailboxRoot)]
pub struct MailboxId(u64);

#[derive(Entity)]
#[domain(
    id = "mailbox-root",
    label = "Mailbox",
    owner = Mailbox,
    actions = [MailboxRootOutputActions]
)]
pub struct MailboxRoot {
    #[domain(identity)]
    id: MailboxId,
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
pub struct Mailbox;

impl domain::AggregateDefinition for Mailbox {
    type Context = Operations;
    type Root = MailboxRoot;
    type Event = MailboxEvents;
}

#[derive(domain::AggregateEvents)]
pub enum MailboxEvents {
    Event0(MailboxOpened),
}

#[derive(DomainIdentity)]
#[domain(owner = DeliveryRoot)]
pub struct DeliveryId(u64);

#[derive(Entity)]
#[domain(id = "delivery-root", label = "Delivery", owner = Delivery)]
pub struct DeliveryRoot {
    #[domain(identity)]
    id: DeliveryId,
}

#[derive(Aggregate)]
#[domain(id = "delivery", label = "Delivery")]
pub struct Delivery;

impl domain::AggregateDefinition for Delivery {
    type Context = Operations;
    type Root = DeliveryRoot;
    type Event = DeliveryEvents;
}

#[derive(domain::AggregateEvents)]
pub enum DeliveryEvents {
    Event0(DeliveryStarted),
}

#[derive(ValueObject)]
#[domain(
    id = "receipt",
    label = "Receipt",
    owner = Operations,
    actions = [ReceiptOutputActions]
)]
pub struct Receipt(String);

#[derive(DomainEvent)]
#[domain(id = "mailbox-opened", label = "Mailbox opened")]
pub struct MailboxOpened;

#[derive(DomainEvent)]
#[domain(id = "delivery-started", label = "Delivery started")]
pub struct DeliveryStarted;

#[derive(DomainError)]
#[domain(id = "mailbox-denied", label = "Mailbox denied", owner = Mailbox, code = "MAILBOX_DENIED", message = "Mailbox denied.")]
pub struct MailboxDenied;

#[domain_actions(aggregate)]
pub trait MailboxOutputActions {
    #[action(id = "event", label = "Event")]
    fn event(root: &mut MailboxRoot) -> MailboxOpened;

    #[action(id = "optional-event", label = "Optional event")]
    fn optional_event(root: &mut MailboxRoot) -> Option<MailboxOpened>;

    #[action(id = "event-list", label = "Event list")]
    fn event_list(root: &mut MailboxRoot) -> Vec<MailboxOpened>;

    #[action(id = "nested-events", label = "Nested events")]
    fn nested_events(root: &mut MailboxRoot) -> Option<Vec<Option<MailboxOpened>>>;

    #[action(id = "fallible-event", label = "Fallible event")]
    fn fallible_event(root: &mut MailboxRoot)
    -> core::result::Result<MailboxOpened, MailboxDenied>;

    #[action(id = "unit", label = "Unit")]
    fn unit(root: &mut MailboxRoot);

    #[action(id = "scalar", label = "Scalar")]
    fn scalar(root: &mut MailboxRoot) -> usize;

    #[action(id = "value-object", label = "Value object")]
    fn value_object(root: &mut MailboxRoot) -> Receipt;

    #[action(id = "optional-unit", label = "Optional unit")]
    fn optional_unit(root: &mut MailboxRoot) -> Option<()>;

    #[action(id = "nested-unit", label = "Nested unit")]
    fn nested_unit(root: &mut MailboxRoot) -> Vec<Option<()>>;
}

impl MailboxOutputActions for Mailbox {
    fn event(root: &mut MailboxRoot) -> MailboxOpened {
        let _ = root;
        MailboxOpened
    }

    fn optional_event(root: &mut MailboxRoot) -> Option<MailboxOpened> {
        let _ = root;
        Some(MailboxOpened)
    }

    fn event_list(root: &mut MailboxRoot) -> Vec<MailboxOpened> {
        let _ = root;
        vec![MailboxOpened]
    }

    fn nested_events(root: &mut MailboxRoot) -> Option<Vec<Option<MailboxOpened>>> {
        let _ = root;
        Some(vec![Some(MailboxOpened)])
    }

    fn fallible_event(
        root: &mut MailboxRoot,
    ) -> core::result::Result<MailboxOpened, MailboxDenied> {
        let _ = root;
        Ok(MailboxOpened)
    }

    fn unit(root: &mut MailboxRoot) {
        let _ = root;
    }

    fn scalar(root: &mut MailboxRoot) -> usize {
        let _ = root;
        1
    }

    fn value_object(root: &mut MailboxRoot) -> Receipt {
        let _ = root;
        Receipt("aggregate".to_owned())
    }

    fn optional_unit(root: &mut MailboxRoot) -> Option<()> {
        let _ = root;
        Some(())
    }

    fn nested_unit(root: &mut MailboxRoot) -> Vec<Option<()>> {
        let _ = root;
        vec![Some(())]
    }
}

#[derive(DomainService)]
#[domain(
    id = "coordinator",
    label = "Coordinator",
    context = Operations,
    actions = [CoordinatorOutputActions]
)]
pub struct Coordinator;

#[derive(DomainError)]
#[domain(id = "coordination-denied", label = "Coordination denied", owner = Coordinator, code = "COORDINATION_DENIED", message = "Coordination denied.")]
pub struct CoordinationDenied;

#[domain_actions(domain_service)]
pub trait CoordinatorOutputActions {
    #[action(id = "unit", label = "Unit")]
    fn unit();

    #[action(id = "scalar", label = "Scalar")]
    fn scalar() -> bool;

    #[action(id = "value-object", label = "Value object")]
    fn value_object() -> Receipt;
}

impl CoordinatorOutputActions for Coordinator {
    fn unit() {}

    fn scalar() -> bool {
        true
    }

    fn value_object() -> Receipt {
        Receipt("service".to_owned())
    }
}

#[domain_actions(entity)]
trait MailboxRootOutputActions {
    #[action(id = "unit", label = "Unit")]
    fn unit(&mut self);

    #[action(id = "scalar", label = "Scalar")]
    fn scalar(&self) -> u64;

    #[action(id = "value-object", label = "Value object")]
    fn value_object(&self) -> Receipt;
}

impl MailboxRootOutputActions for MailboxRoot {
    fn unit(&mut self) {}

    fn scalar(&self) -> u64 {
        self.id.0
    }

    fn value_object(&self) -> Receipt {
        Receipt(self.id.0.to_string())
    }
}

#[domain_actions(value_object)]
trait ReceiptOutputActions {
    #[action(id = "new", label = "New")]
    fn new(input: String) -> Self;

    #[action(id = "replace", label = "Replace")]
    fn replace(self, input: String) -> Self;
}

impl ReceiptOutputActions for Receipt {
    fn new(input: String) -> Self {
        Self(input)
    }

    fn replace(self, input: String) -> Self {
        let _ = self;
        Self(input)
    }
}

#[test]
fn permits_owned_events_and_existing_output_kinds() {
    let mailbox_actions = <Mailbox as MailboxOutputActions>::__DOMAIN_ACTIONS;
    let coordinator_actions = <Coordinator as CoordinatorOutputActions>::__DOMAIN_ACTIONS;
    let mailbox_root_actions = <MailboxRoot as MailboxRootOutputActions>::__DOMAIN_ACTIONS;
    let receipt_actions = <Receipt as ReceiptOutputActions>::__DOMAIN_ACTIONS;
    let event = ActionOutputDescriptor::DomainEvent(
        <MailboxOpened as DomainEventType<Mailbox>>::DESCRIPTOR.id,
    );
    assert_eq!(mailbox_actions[0].output, Some(event));
    assert_eq!(
        mailbox_actions[1].output,
        Some(ActionOutputDescriptor::Optional(
            &ActionOutputDescriptor::DomainEvent(
                <MailboxOpened as DomainEventType<Mailbox>>::DESCRIPTOR.id,
            ),
        ))
    );
    assert_eq!(
        mailbox_actions[2].output,
        Some(ActionOutputDescriptor::List(
            &ActionOutputDescriptor::DomainEvent(
                <MailboxOpened as DomainEventType<Mailbox>>::DESCRIPTOR.id,
            ),
        ))
    );
    assert!(matches!(
        mailbox_actions[3].output,
        Some(ActionOutputDescriptor::Optional(_))
    ));
    assert_eq!(mailbox_actions[4].output, Some(event));
    assert_eq!(mailbox_actions[4].error, Some(MailboxDenied::DESCRIPTOR.id));
    assert_eq!(mailbox_actions[5].output, None);
    assert_eq!(
        mailbox_actions[6].output,
        Some(ActionOutputDescriptor::Scalar(ScalarType::Usize))
    );
    assert_eq!(
        mailbox_actions[7].output,
        Some(ActionOutputDescriptor::ValueObject(Receipt::DESCRIPTOR.id))
    );
    assert_eq!(mailbox_actions[8].output, None);
    assert_eq!(mailbox_actions[9].output, None);

    assert_eq!(coordinator_actions[0].output, None);
    assert!(matches!(
        coordinator_actions[1].output,
        Some(ActionOutputDescriptor::Scalar(ScalarType::Bool))
    ));
    assert!(matches!(
        coordinator_actions[2].output,
        Some(ActionOutputDescriptor::ValueObject(_))
    ));
    assert_eq!(mailbox_root_actions[0].output, None);
    assert!(matches!(
        mailbox_root_actions[1].output,
        Some(ActionOutputDescriptor::Scalar(ScalarType::U64))
    ));
    assert!(matches!(
        mailbox_root_actions[2].output,
        Some(ActionOutputDescriptor::ValueObject(_))
    ));
    assert!(receipt_actions.iter().all(|action| {
        action.output == Some(ActionOutputDescriptor::ValueObject(Receipt::DESCRIPTOR.id))
    }));
}

#[test]
fn projects_only_explicitly_attached_non_aggregate_actions() {
    let model = domain_model! {
        contexts: [Operations],
        aggregates: [Mailbox, Delivery],
        entities: [MailboxRoot, DeliveryRoot],
        identities: [MailboxId, DeliveryId],
        value_objects: [Receipt],
        services: [Coordinator],
        commands: [],
        errors: [MailboxDenied, CoordinationDenied],

        query_groups: [],
    }
    .expect("action output domain model should be valid");

    assert_eq!(
        model["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "unit",
            "scalar",
            "value-object",
            "new",
            "replace",
            "unit",
            "scalar",
            "value-object",
        ]
    );
}
