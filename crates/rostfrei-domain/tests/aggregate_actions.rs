#![allow(dead_code)]

use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    Aggregate, AggregateType, BoundedContext, Command, DomainError, DomainErrorType, DomainEvent,
    DomainEventType, DomainIdentity, Entity, ValueObject, ValueObjectType, domain_actions,
    domain_model,
};

#[derive(BoundedContext)]
#[domain(id = "accounts", label = "Accounts")]
pub struct Accounts;

#[derive(DomainIdentity)]
#[domain(owner = AccountRoot)]
pub struct AccountId(u64);

#[derive(Entity)]
#[domain(
    id = "account-root",
    label = "Account root",
    owner = Account,
    actions = [AccountRootActions]
)]
pub struct AccountRoot {
    #[domain(identity)]
    id: AccountId,
    revision: u32,
    name: String,
}

pub type AccountRootAlias = AccountRoot;

#[domain_actions(entity)]
trait AccountRootActions {
    #[action(id = "touch-root", label = "Touch account root")]
    fn touch_root(&mut self);
}

impl AccountRootActions for AccountRoot {
    fn touch_root(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

#[derive(Aggregate)]
#[domain(id = "account", label = "Account")]
pub struct Account;

impl domain::AggregateDefinition for Account {
    type Context = Accounts;
    type Root = AccountRoot;
    type Event = AccountEvents;
}

#[derive(domain::AggregateEvents)]
pub enum AccountEvents {
    Event0(AccountChanged),
}

#[derive(Command)]
#[domain(id = "rename-account", label = "Rename account", owner = Account)]
pub struct RenameAccount;

#[derive(ValueObject)]
#[domain(id = "rename-account-input", label = "Rename account input", owner = Account)]
pub struct RenameAccountInput;

#[derive(DomainEvent, Debug, Eq, PartialEq)]
#[domain(id = "account-changed", label = "Account changed")]
pub struct AccountChanged;

#[derive(DomainError, Debug, Eq, PartialEq)]
#[domain(
    id = "account-denied",
    label = "Account denied",
    owner = Account,
    code = "ACCOUNT_DENIED",
    message = "The account change was denied."
)]
pub struct AccountDenied;

mod contracts {
    use domain::domain_actions;

    #[domain_actions(aggregate)]
    pub trait AccountLifecycle {
        #[action(id = "open", label = "Open account")]
        fn open(root: &mut super::AccountRoot) -> super::AccountChanged;

        #[action(id = "rename", label = "Rename account")]
        fn rename(
            root: &mut super::AccountRootAlias,
            input: super::RenameAccountInput,
        ) -> Result<super::AccountChanged, super::AccountDenied>;
    }

    #[domain_actions(aggregate)]
    pub trait AccountMaintenance {
        #[action(id = "freeze", label = "Freeze account")]
        fn freeze(root: &mut super::AccountRoot);

        #[action(id = "revision", label = "Account revision")]
        fn revision(root: &mut super::AccountRootAlias) -> u32;
    }

    #[domain_actions(aggregate)]
    pub trait ImplementedOnly {
        #[action(id = "implemented-only", label = "Implemented only")]
        fn implemented_only(root: &mut super::AccountRoot);
    }
}

impl contracts::AccountLifecycle for Account {
    fn open(root: &mut AccountRoot) -> AccountChanged {
        root.revision = root.revision.saturating_add(1);
        AccountChanged
    }

    fn rename(
        root: &mut AccountRootAlias,
        _input: RenameAccountInput,
    ) -> Result<AccountChanged, AccountDenied> {
        let revision = root.revision.checked_add(1).ok_or(AccountDenied)?;
        "Renamed".clone_into(&mut root.name);
        root.revision = revision;
        Ok(AccountChanged)
    }
}

impl contracts::AccountMaintenance for Account {
    fn freeze(root: &mut AccountRoot) {
        root.revision = root.revision.saturating_add(1);
    }

    fn revision(root: &mut AccountRootAlias) -> u32 {
        root.revision
    }
}

impl contracts::ImplementedOnly for Account {
    fn implemented_only(root: &mut AccountRoot) {
        root.revision = root.revision.saturating_add(1);
    }
}

struct AccountExtensionActions;

impl ActionGroupType for AccountExtensionActions {
    type Owner = Account;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Account::DESCRIPTOR.id),
            local: "extension",
        },
        label: "Account extension action",
        input: None,
        output: None,
        raises: &[],
        error: None,
    }];
}

struct DuplicateAccountExtensionActions;

impl ActionGroupType for DuplicateAccountExtensionActions {
    type Owner = Account;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Account::DESCRIPTOR.id),
            local: "open",
        },
        label: "Duplicate open",
        input: None,
        output: None,
        raises: &[],
        error: None,
    }];
}

#[derive(DomainIdentity)]
#[domain(owner = UnlistedRoot)]
pub struct UnlistedId(u64);

#[derive(Entity)]
#[domain(id = "unlisted-root", label = "Unlisted root", owner = UnlistedAggregate)]
pub struct UnlistedRoot {
    #[domain(identity)]
    id: UnlistedId,
}

#[derive(Aggregate)]
#[domain(id = "unlisted", label = "Unlisted")]
pub struct UnlistedAggregate;

impl domain::AggregateDefinition for UnlistedAggregate {
    type Context = Accounts;
    type Root = UnlistedRoot;
    type Event = domain::NoDomainEvents;
}

#[domain_actions(aggregate)]
pub trait UnlistedActions {
    #[action(id = "unlisted-action", label = "Unlisted action")]
    fn unlisted_action(root: &mut UnlistedRoot);
}

impl UnlistedActions for UnlistedAggregate {
    fn unlisted_action(_root: &mut UnlistedRoot) {}
}

#[derive(DomainIdentity)]
#[domain(owner = OmittedActionsRoot)]
struct OmittedActionsId(u64);

#[derive(Entity)]
#[domain(
    id = "omitted-actions-root",
    label = "Omitted actions root",
    owner = OmittedActionsAggregate
)]
struct OmittedActionsRoot {
    #[domain(identity)]
    id: OmittedActionsId,
}

#[derive(Aggregate)]
#[domain(id = "omitted-actions", label = "Omitted actions")]
struct OmittedActionsAggregate;

impl domain::AggregateDefinition for OmittedActionsAggregate {
    type Context = Accounts;
    type Root = OmittedActionsRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(DomainIdentity)]
#[domain(owner = EmptyActionsRoot)]
struct EmptyActionsId(u64);

#[derive(Entity)]
#[domain(
    id = "empty-actions-root",
    label = "Empty actions root",
    owner = EmptyActionsAggregate
)]
struct EmptyActionsRoot {
    #[domain(identity)]
    id: EmptyActionsId,
}

#[derive(Aggregate)]
#[domain(id = "empty-actions", label = "Empty actions")]
struct EmptyActionsAggregate;

impl domain::AggregateDefinition for EmptyActionsAggregate {
    type Context = Accounts;
    type Root = EmptyActionsRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(DomainIdentity)]
#[domain(owner = DuplicateRoot)]
pub struct DuplicateId(u64);

#[derive(Entity)]
#[domain(id = "duplicate-root", label = "Duplicate root", owner = DuplicateAggregate)]
pub struct DuplicateRoot {
    #[domain(identity)]
    id: DuplicateId,
}

#[derive(Aggregate)]
#[domain(id = "duplicate", label = "Duplicate")]
pub struct DuplicateAggregate;

impl domain::AggregateDefinition for DuplicateAggregate {
    type Context = Accounts;
    type Root = DuplicateRoot;
    type Event = domain::NoDomainEvents;
}

#[domain_actions(aggregate)]
pub trait FirstDuplicateActions {
    #[action(id = "duplicate", label = "First duplicate")]
    fn first(root: &mut DuplicateRoot);
}

#[domain_actions(aggregate)]
pub trait SecondDuplicateActions {
    #[action(id = "duplicate", label = "Second duplicate")]
    fn second(root: &mut DuplicateRoot);
}

impl FirstDuplicateActions for DuplicateAggregate {
    fn first(_root: &mut DuplicateRoot) {}
}

impl SecondDuplicateActions for DuplicateAggregate {
    fn second(_root: &mut DuplicateRoot) {}
}

fn account_root() -> AccountRoot {
    AccountRoot {
        id: AccountId(7),
        revision: 0,
        name: "Initial".to_owned(),
    }
}

#[test]
fn public_aggregate_contracts_are_invocable_with_concrete_and_aliased_roots() {
    let mut root = account_root();

    assert_eq!(
        <Account as contracts::AccountLifecycle>::open(&mut root),
        AccountChanged
    );
    assert_eq!(
        <Account as contracts::AccountLifecycle>::rename(&mut root, RenameAccountInput),
        Ok(AccountChanged)
    );
    <Account as contracts::AccountMaintenance>::freeze(&mut root);

    assert_eq!(
        <Account as contracts::AccountMaintenance>::revision(&mut root),
        3
    );
    assert_eq!(root.name, "Renamed");
}

#[test]
fn aggregate_action_contracts_preserve_method_order_and_descriptors() {
    let contracts = [
        <Account as contracts::AccountLifecycle>::__DOMAIN_ACTIONS,
        <Account as contracts::AccountMaintenance>::__DOMAIN_ACTIONS,
    ];
    assert_eq!(contracts.len(), 2);
    assert_eq!(
        contracts[0]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["open", "rename"]
    );
    assert_eq!(
        contracts[1]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["freeze", "revision"]
    );

    assert_eq!(contracts[0][0].input, None);
    assert_eq!(
        contracts[0][0].output,
        Some(ActionOutputDescriptor::DomainEvent(
            <AccountChanged as DomainEventType<Account>>::DESCRIPTOR.id
        ))
    );
    assert_eq!(contracts[0][0].error, None);
    assert_eq!(
        contracts[0][1].input,
        Some(ActionInputDescriptor::ValueObject(
            RenameAccountInput::DESCRIPTOR.id
        ))
    );
    assert_eq!(
        contracts[0][1].output,
        Some(ActionOutputDescriptor::DomainEvent(
            <AccountChanged as DomainEventType<Account>>::DESCRIPTOR.id
        ))
    );
    assert_eq!(contracts[0][1].error, Some(AccountDenied::DESCRIPTOR.id));

    let implemented_only = <Account as contracts::ImplementedOnly>::__DOMAIN_ACTIONS;
    assert_eq!(implemented_only[0].id.local, "implemented-only");
    assert!(
        contracts
            .iter()
            .flat_map(|contract| contract.iter())
            .all(|action| action.id.local != "implemented-only")
    );
    assert!(<OmittedActionsAggregate as AggregateType>::ACTION_CONTRACTS.is_empty());
    assert!(<EmptyActionsAggregate as AggregateType>::ACTION_CONTRACTS.is_empty());
}

#[test]
fn model_projects_explicit_extensions_and_non_aggregate_attachments() {
    let model = domain_model! {
        contexts: [Accounts],
        aggregates: [Account, OmittedActionsAggregate, EmptyActionsAggregate],
        entities: [AccountRoot],
        identities: [AccountId],
        value_objects: [RenameAccountInput],
        services: [],
        commands: [RenameAccount],
        errors: [AccountDenied],
        action_extensions: [AccountExtensionActions],
        query_groups: [],
    }
    .expect("aggregate action domain model should be valid");

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["touch-root", "extension"]
    );
    assert_eq!(actions[0]["id"]["owner"]["kind"], "entity");
    assert_eq!(actions[1]["id"]["owner"]["kind"], "aggregate");
    assert!(actions.iter().all(|action| {
        action["id"]["local"] != "implemented-only" && action["id"]["local"] != "unlisted-action"
    }));
    assert!(
        model["aggregates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|aggregate| { aggregate["id"]["local"] != "unlisted" })
    );
}

#[test]
fn unattached_duplicate_aggregate_traits_are_not_registered() {
    let model = domain_model! {
        contexts: [],
        aggregates: [DuplicateAggregate],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
    .expect("unattached aggregate action traits do not enter the model");
    assert!(model["actions"].as_array().unwrap().is_empty());
}

#[test]
fn extension_is_registered_when_same_named_contract_is_unattached() {
    let model = domain_model! {
        contexts: [],
        aggregates: [Account],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        action_extensions: [DuplicateAccountExtensionActions],
        query_groups: [],
    }
    .expect("unattached action traits do not conflict with explicit extensions");
    assert_eq!(model["actions"][0]["id"]["local"], "open");
}
