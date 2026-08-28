#![allow(dead_code)]

use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    Aggregate, AggregateType, BoundedContext, DomainCommand, DomainError, DomainErrorType,
    DomainEvent, DomainEventType, DomainIdentity, Entity, ValueObject, ValueObjectType,
    domain_actions, domain_model,
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
#[domain(
    id = "account",
    label = "Account",
    context = Accounts,
    root = AccountRoot,
    actions = [contracts::AccountLifecycle, contracts::AccountMaintenance],
    events = [AccountChanged]
)]
pub struct Account;

#[derive(DomainCommand)]
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
#[domain(
    id = "unlisted",
    label = "Unlisted",
    context = Accounts,
    root = UnlistedRoot,
    actions = [UnlistedActions]
)]
pub struct UnlistedAggregate;

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
#[domain(
    id = "omitted-actions",
    label = "Omitted actions",
    context = Accounts,
    root = OmittedActionsRoot
)]
struct OmittedActionsAggregate;

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
#[domain(
    id = "empty-actions",
    label = "Empty actions",
    context = Accounts,
    root = EmptyActionsRoot,
    actions = []
)]
struct EmptyActionsAggregate;

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
#[domain(
    id = "duplicate",
    label = "Duplicate",
    context = Accounts,
    root = DuplicateRoot,
    actions = [FirstDuplicateActions, SecondDuplicateActions]
)]
pub struct DuplicateAggregate;

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
fn aggregate_action_contracts_preserve_attachment_and_method_order_and_descriptors() {
    let contracts = <Account as AggregateType>::ACTION_CONTRACTS;
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
            AccountChanged::DESCRIPTOR.id
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
            AccountChanged::DESCRIPTOR.id
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
fn model_projects_attached_then_extension_actions_and_omits_unlisted_contracts() {
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
    };

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "open",
            "rename",
            "freeze",
            "revision",
            "touch-root",
            "extension"
        ]
    );
    assert_eq!(actions[0]["id"]["owner"]["kind"], "aggregate");
    assert_eq!(actions[3]["id"]["owner"]["kind"], "aggregate");
    assert_eq!(actions[4]["id"]["owner"]["kind"], "entity");
    assert_eq!(actions[5]["id"]["owner"]["kind"], "aggregate");
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
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_across_attached_aggregate_traits() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [DuplicateAggregate],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    };
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_between_attached_and_extension_groups() {
    let _ = domain_model! {
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
    };
}
