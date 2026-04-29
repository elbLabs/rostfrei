#![allow(dead_code)]

use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, Aggregate, AggregateType, BoundedContext, Command,
    CommandOwnerId, CommandType, DomainError, DomainErrorOwnerId, DomainErrorType, DomainEvent,
    DomainIdentity, DomainModelError, DomainService, DomainServiceType, Entity, ValueObject,
    domain_actions, domain_model,
};

#[derive(BoundedContext)]
#[domain(id = "operations", label = "Operations")]
pub struct Operations;

#[derive(DomainIdentity)]
pub struct WorkId(u64);

#[domain_actions(entity)]
trait WorkRootActions {
    #[action(id = "inspect-work", label = "Inspect work")]
    fn inspect_work(&self) -> bool;
}

#[derive(Entity)]
#[domain(id = "work-root", label = "Work root")]
pub struct WorkRoot {
    #[domain(identity)]
    id: WorkId,
    active: bool,
}

impl domain::EntityDefinition for WorkRoot {
    type Owner = Work;
    type Identity = WorkId;
}

impl WorkRootActions for WorkRoot {
    fn inspect_work(&self) -> bool {
        self.active
    }
}

#[domain_actions(aggregate)]
pub trait WorkActions {
    #[action(id = "start-work", label = "Start work")]
    fn start_work(root: &mut WorkRoot) -> WorkStarted;
}

#[derive(Aggregate)]
#[domain(id = "work", label = "Work")]
pub struct Work;

impl domain::AggregateDefinition for Work {
    type Context = Operations;
    type Root = WorkRoot;
    type Event = WorkEvents;
}

#[derive(domain::AggregateEvents)]
pub enum WorkEvents {
    Event0(WorkStarted),
}

impl WorkActions for Work {
    fn start_work(root: &mut WorkRoot) -> WorkStarted {
        root.active = true;
        WorkStarted
    }
}

#[derive(DomainEvent, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "work-started", label = "Work started")]
pub struct WorkStarted;

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "receipt", label = "Receipt")]
pub struct Receipt(u64);

#[derive(Command, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "coordinate-work", label = "Coordinate work", owner = Coordinator)]
pub struct CoordinateWork;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoordinateWorkInput;

#[derive(DomainError, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "coordination-failed",
    label = "Coordination failed",
    owner = Coordinator,
    code = "COORDINATION_FAILED",
    message = "Coordination failed."
)]
pub struct CoordinationFailed;

mod contracts {
    use domain::domain_actions;

    #[domain_actions(domain_service)]
    pub trait Coordination {
        #[action(id = "available", label = "Coordination available")]
        fn available() -> bool;

        #[action(id = "coordinate", label = "Coordinate work")]
        fn coordinate(
            input: super::CoordinateWorkInput,
        ) -> Result<super::Receipt, super::CoordinationFailed>;

        #[action(id = "planned-receipts", label = "Planned receipts")]
        fn planned_receipts() -> Option<Vec<Option<super::Receipt>>>;
    }
}

#[domain_actions(domain_service)]
pub trait CoordinationReporting {
    #[action(id = "receipt", label = "Coordination receipt")]
    fn receipt() -> Receipt;
}

#[domain_actions(domain_service)]
pub trait UnattachedCoordination {
    #[action(id = "unattached", label = "Unattached coordination")]
    fn unattached() -> u8;
}

#[derive(DomainService)]
#[domain(
    id = "coordinator",
    label = "Coordinator",
    context = Operations,
    actions = [contracts::Coordination, CoordinationReporting]
)]
pub struct Coordinator;

impl contracts::Coordination for Coordinator {
    fn available() -> bool {
        true
    }

    fn coordinate(_input: CoordinateWorkInput) -> Result<Receipt, CoordinationFailed> {
        Ok(Receipt(42))
    }

    fn planned_receipts() -> Option<Vec<Option<Receipt>>> {
        Some(vec![Some(Receipt(42))])
    }
}

impl CoordinationReporting for Coordinator {
    fn receipt() -> Receipt {
        Receipt(42)
    }
}

impl UnattachedCoordination for Coordinator {
    fn unattached() -> u8 {
        7
    }
}

#[derive(DomainService)]
#[domain(id = "omitted-actions", label = "Omitted actions", context = Operations)]
struct OmittedActionsService;

#[derive(DomainService)]
#[domain(
    id = "empty-actions",
    label = "Empty actions",
    context = Operations,
    actions = []
)]
struct EmptyActionsService;

#[domain_actions(domain_service)]
pub trait FirstDuplicateActions {
    #[action(id = "duplicate", label = "First duplicate")]
    fn first();
}

#[domain_actions(domain_service)]
pub trait SecondDuplicateActions {
    #[action(id = "duplicate", label = "Second duplicate")]
    fn second();
}

#[derive(DomainService)]
#[domain(
    id = "duplicate-service",
    label = "Duplicate service",
    context = Operations,
    actions = [FirstDuplicateActions, SecondDuplicateActions]
)]
struct DuplicateService;

impl FirstDuplicateActions for DuplicateService {
    fn first() {}
}

impl SecondDuplicateActions for DuplicateService {
    fn second() {}
}

struct WorkExtensionActions;

impl ActionGroupType for WorkExtensionActions {
    type Owner = Work;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Work::DESCRIPTOR.id),
            local: "work-extension",
        },
        label: "Work extension",
        error: None,
    }];
}

struct DuplicateCoordinatorExtensionActions;

impl ActionGroupType for DuplicateCoordinatorExtensionActions {
    type Owner = Coordinator;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::DomainService(Coordinator::DESCRIPTOR.id),
            local: "available",
        },
        label: "Duplicate available",
        error: None,
    }];
}

#[test]
fn public_domain_service_contracts_are_invocable_with_zero_and_one_input() {
    assert!(<Coordinator as contracts::Coordination>::available());
    assert_eq!(
        <Coordinator as contracts::Coordination>::coordinate(CoordinateWorkInput),
        Ok(Receipt(42))
    );
    assert_eq!(
        <Coordinator as contracts::Coordination>::planned_receipts(),
        Some(vec![Some(Receipt(42))])
    );
    assert_eq!(
        <Coordinator as CoordinationReporting>::receipt(),
        Receipt(42)
    );
    assert_eq!(<Coordinator as UnattachedCoordination>::unattached(), 7);
}

#[test]
fn domain_service_action_contracts_preserve_attachments_order_and_descriptors() {
    let contracts = <Coordinator as DomainServiceType>::ACTION_CONTRACTS;

    assert_eq!(contracts.len(), 2);
    assert_eq!(
        contracts[0],
        <Coordinator as contracts::Coordination>::__DOMAIN_ACTIONS
    );
    assert_eq!(
        contracts[1],
        <Coordinator as CoordinationReporting>::__DOMAIN_ACTIONS
    );
    assert_eq!(
        contracts[0]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["available", "coordinate", "planned-receipts"]
    );
    assert_eq!(
        contracts[1]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["receipt"]
    );
    assert_eq!(
        CoordinateWork::DESCRIPTOR.id.owner,
        CommandOwnerId::DomainService(Coordinator::DESCRIPTOR.id)
    );
    assert_eq!(
        CoordinationFailed::DESCRIPTOR.id.owner,
        DomainErrorOwnerId::DomainService(Coordinator::DESCRIPTOR.id)
    );
    assert_eq!(
        contracts[0][1].error,
        Some(CoordinationFailed::DESCRIPTOR.id)
    );
    assert_eq!(
        <Coordinator as UnattachedCoordination>::__DOMAIN_ACTIONS[0]
            .id
            .local,
        "unattached"
    );
    assert!(<OmittedActionsService as DomainServiceType>::ACTION_CONTRACTS.is_empty());
    assert!(<EmptyActionsService as DomainServiceType>::ACTION_CONTRACTS.is_empty());
}

#[test]
fn model_orders_attached_then_extension_actions_across_owner_kinds() {
    let model = domain_model! {
        contexts: [Operations],
        aggregates: [Work],
        entities: [WorkRoot],
        value_objects: [Receipt],
        services: [Coordinator, OmittedActionsService, EmptyActionsService],
        commands: [CoordinateWork],
        errors: [CoordinationFailed],
        action_extensions: [WorkExtensionActions],
        query_groups: [],
    }
    .expect("domain-service action model should be valid");

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "available",
            "coordinate",
            "planned-receipts",
            "receipt",
            "work-extension",
        ]
    );
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["owner"]["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "domainService",
            "domainService",
            "domainService",
            "domainService",
            "aggregate",
        ]
    );
    assert!(
        actions
            .iter()
            .all(|action| action["id"]["local"] != "unattached")
    );
}

#[test]
fn rejects_duplicate_action_id_across_attached_domain_service_contracts() {
    let error = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [DuplicateService],
        commands: [],
        errors: [],
        query_groups: [],
    }
    .expect_err("duplicate attached domain-service actions should be rejected");
    let id = ActionId {
        owner: ActionOwnerId::DomainService(DuplicateService::DESCRIPTOR.id),
        local: "duplicate",
    };

    assert_eq!(
        error,
        DomainModelError::DuplicateActionId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate ActionId: {id:?}"));
}

#[test]
fn rejects_duplicate_action_id_between_attached_and_extension_domain_service_groups() {
    let error = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [Coordinator],
        commands: [],
        errors: [],
        action_extensions: [DuplicateCoordinatorExtensionActions],
        query_groups: [],
    }
    .expect_err("an extension duplicating an attached domain-service action should be rejected");
    let id = ActionId {
        owner: ActionOwnerId::DomainService(Coordinator::DESCRIPTOR.id),
        local: "available",
    };

    assert_eq!(
        error,
        DomainModelError::DuplicateActionId { id: Box::new(id) }
    );
    assert_eq!(error.to_string(), format!("duplicate ActionId: {id:?}"));
}
