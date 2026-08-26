#![allow(dead_code)]

use domain::extension::ActionGroupType;
use domain::{
    ActionDescriptor, ActionId, ActionInputDescriptor, ActionOutputDescriptor, ActionOwnerId,
    Aggregate, AggregateType, BoundedContext, DomainError, DomainErrorType, DomainIdentity, Entity,
    ScalarType, ValueObject, ValueObjectType, domain_actions, domain_model,
};

#[derive(BoundedContext)]
#[domain(id = "billing", label = "Billing")]
pub struct Billing;

#[derive(DomainIdentity)]
#[domain(owner = LedgerRoot)]
pub struct LedgerId(u64);

#[domain_actions(entity)]
trait LedgerRootActions {
    #[action(id = "root-state", label = "Ledger root state")]
    fn root_state(&self) -> bool;
}

#[derive(Entity)]
#[domain(
    id = "ledger-root",
    label = "Ledger root",
    owner = Ledger,
    actions = [LedgerRootActions]
)]
pub struct LedgerRoot {
    #[domain(identity)]
    id: LedgerId,
    active: bool,
}

impl LedgerRootActions for LedgerRoot {
    fn root_state(&self) -> bool {
        self.active
    }
}

#[domain_actions(aggregate)]
pub trait LedgerActions {
    #[action(id = "ledger-command", label = "Ledger command")]
    fn ledger_command(root: &mut LedgerRoot);
}

#[derive(Aggregate)]
#[domain(
    id = "ledger",
    label = "Ledger",
    context = Billing,
    root = LedgerRoot,
    actions = [LedgerActions]
)]
pub struct Ledger;

impl LedgerActions for Ledger {
    fn ledger_command(root: &mut LedgerRoot) {
        root.active = true;
    }
}

#[domain_actions(value_object)]
trait MoneyConstruction {
    #[action(id = "from-minor", label = "From minor units")]
    fn from_minor(input: u64) -> Self;

    #[action(id = "clear", label = "Clear money")]
    fn clear(self) -> Self;
}

mod contracts {
    use domain::domain_actions;

    #[domain_actions(value_object)]
    pub(crate) trait MoneyArithmetic {
        #[action(id = "increase", label = "Increase money")]
        fn increase(self, input: u64) -> super::MoneyAlias;

        #[action(id = "checked-increase", label = "Checked increase")]
        fn checked_increase(self, input: u64) -> Result<Self, super::MoneyOverflow>;
    }
}

#[domain_actions(value_object)]
trait UnattachedMoneyActions {
    #[action(id = "unattached", label = "Unattached money action")]
    fn unattached(self) -> Self;
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "money",
    label = "Money",
    owner = Ledger,
    actions = [MoneyConstruction, contracts::MoneyArithmetic]
)]
struct Money(u64);

type MoneyAlias = Money;

#[derive(DomainError, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "money-overflow",
    label = "Money overflow",
    owner = Money,
    code = "MONEY_OVERFLOW",
    message = "The money amount overflowed."
)]
struct MoneyOverflow;

impl MoneyConstruction for Money {
    fn from_minor(input: u64) -> Self {
        Self(input)
    }

    fn clear(self) -> Self {
        Self(0)
    }
}

impl contracts::MoneyArithmetic for Money {
    fn increase(self, input: u64) -> MoneyAlias {
        Self(self.0 + input)
    }

    fn checked_increase(self, input: u64) -> Result<Self, MoneyOverflow> {
        self.0.checked_add(input).map(Self).ok_or(MoneyOverflow)
    }
}

impl UnattachedMoneyActions for Money {
    fn unattached(self) -> Self {
        self
    }
}

#[derive(ValueObject)]
#[domain(id = "omitted-actions", label = "Omitted actions", owner = Ledger)]
struct OmittedActionsValue(u8);

#[derive(ValueObject)]
#[domain(
    id = "empty-actions",
    label = "Empty actions",
    owner = Ledger,
    actions = []
)]
struct EmptyActionsValue(u8);

#[domain_actions(value_object)]
trait UnlistedValueActions {
    #[action(id = "unlisted", label = "Unlisted value action")]
    fn unlisted(self) -> Self;
}

#[derive(ValueObject)]
#[domain(
    id = "unlisted-value",
    label = "Unlisted value",
    owner = Ledger,
    actions = [UnlistedValueActions]
)]
struct UnlistedValue(u8);

impl UnlistedValueActions for UnlistedValue {
    fn unlisted(self) -> Self {
        self
    }
}

struct LedgerExtensionActions;

impl ActionGroupType for LedgerExtensionActions {
    type Owner = Ledger;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::Aggregate(Ledger::DESCRIPTOR.id),
            local: "ledger-extension",
        },
        label: "Ledger extension action",
        input: None,
        output: None,
        error: None,
    }];
}

struct DuplicateMoneyExtensionActions;

impl ActionGroupType for DuplicateMoneyExtensionActions {
    type Owner = Money;

    const ACTIONS: &'static [ActionDescriptor] = &[ActionDescriptor {
        id: ActionId {
            owner: ActionOwnerId::ValueObject(Money::DESCRIPTOR.id),
            local: "from-minor",
        },
        label: "Duplicate from minor units",
        input: Some(ActionInputDescriptor::Scalar(ScalarType::U64)),
        output: Some(ActionOutputDescriptor::ValueObject(Money::DESCRIPTOR.id)),
        error: None,
    }];
}

#[domain_actions(value_object)]
trait FirstDuplicateActions {
    #[action(id = "duplicate", label = "First duplicate")]
    fn first(input: u8) -> Self;
}

#[domain_actions(value_object)]
trait SecondDuplicateActions {
    #[action(id = "duplicate", label = "Second duplicate")]
    fn second(input: u8) -> Self;
}

#[derive(ValueObject)]
#[domain(
    id = "duplicate-value",
    label = "Duplicate value",
    owner = Ledger,
    actions = [FirstDuplicateActions, SecondDuplicateActions]
)]
struct DuplicateValue(u8);

impl FirstDuplicateActions for DuplicateValue {
    fn first(input: u8) -> Self {
        Self(input)
    }
}

impl SecondDuplicateActions for DuplicateValue {
    fn second(input: u8) -> Self {
        Self(input)
    }
}

fn ledger_root() -> LedgerRoot {
    LedgerRoot {
        id: LedgerId(7),
        active: false,
    }
}

#[test]
fn restricted_and_inherited_contracts_are_invocable_with_supported_arities_and_outputs() {
    use contracts::MoneyArithmetic as _;

    let money = <Money as MoneyConstruction>::from_minor(10);
    assert_eq!(money.increase(5), Money(15));
    assert_eq!(Money(9).clear(), Money(0));
    assert_eq!(Money(7).checked_increase(8), Ok(Money(15)));
    assert_eq!(Money(u64::MAX).checked_increase(1), Err(MoneyOverflow));
    assert_eq!(Money(3).unattached(), Money(3));
}

#[test]
fn value_object_action_contracts_preserve_attachment_and_method_order_and_descriptor_shape() {
    let contracts = <Money as ValueObjectType>::ACTION_CONTRACTS;
    let owner = ActionOwnerId::ValueObject(Money::DESCRIPTOR.id);

    assert_eq!(contracts.len(), 2);
    assert_eq!(contracts[0], <Money as MoneyConstruction>::__DOMAIN_ACTIONS);
    assert_eq!(
        contracts[1],
        <Money as contracts::MoneyArithmetic>::__DOMAIN_ACTIONS
    );
    assert_eq!(
        contracts[0],
        &[
            ActionDescriptor {
                id: ActionId {
                    owner,
                    local: "from-minor",
                },
                label: "From minor units",
                input: Some(ActionInputDescriptor::Scalar(ScalarType::U64)),
                output: Some(ActionOutputDescriptor::ValueObject(Money::DESCRIPTOR.id)),
                error: None,
            },
            ActionDescriptor {
                id: ActionId {
                    owner,
                    local: "clear",
                },
                label: "Clear money",
                input: None,
                output: Some(ActionOutputDescriptor::ValueObject(Money::DESCRIPTOR.id)),
                error: None,
            },
        ]
    );
    assert_eq!(
        contracts[1],
        &[
            ActionDescriptor {
                id: ActionId {
                    owner,
                    local: "increase",
                },
                label: "Increase money",
                input: Some(ActionInputDescriptor::Scalar(ScalarType::U64)),
                output: Some(ActionOutputDescriptor::ValueObject(Money::DESCRIPTOR.id)),
                error: None,
            },
            ActionDescriptor {
                id: ActionId {
                    owner,
                    local: "checked-increase",
                },
                label: "Checked increase",
                input: Some(ActionInputDescriptor::Scalar(ScalarType::U64)),
                output: Some(ActionOutputDescriptor::ValueObject(Money::DESCRIPTOR.id)),
                error: Some(MoneyOverflow::DESCRIPTOR.id),
            },
        ]
    );
    assert_eq!(
        <Money as UnattachedMoneyActions>::__DOMAIN_ACTIONS[0]
            .id
            .local,
        "unattached"
    );
    assert!(<OmittedActionsValue as ValueObjectType>::ACTION_CONTRACTS.is_empty());
    assert!(<EmptyActionsValue as ValueObjectType>::ACTION_CONTRACTS.is_empty());
}

#[test]
fn model_orders_attached_then_extension_actions_and_omits_unlisted_contracts() {
    let model = domain_model! {
        contexts: [Billing],
        aggregates: [Ledger],
        entities: [LedgerRoot],
        identities: [LedgerId],
        value_objects: [Money, OmittedActionsValue, EmptyActionsValue],
        services: [],
        commands: [],
        events: [],
        errors: [MoneyOverflow],
        action_extensions: [LedgerExtensionActions],
        query_groups: [],
    };

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "ledger-command",
            "root-state",
            "from-minor",
            "clear",
            "increase",
            "checked-increase",
            "ledger-extension",
        ]
    );
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["owner"]["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "aggregate",
            "entity",
            "valueObject",
            "valueObject",
            "valueObject",
            "valueObject",
            "aggregate",
        ]
    );
    assert!(actions.iter().all(|action| {
        action["id"]["local"] != "unattached" && action["id"]["local"] != "unlisted"
    }));
    assert!(
        model["valueObjects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value["id"]["local"] != "unlisted-value")
    );
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_across_attached_value_object_contracts() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [DuplicateValue],
        services: [],
        commands: [],
        events: [],
        errors: [],
        query_groups: [],
    };
}

#[test]
#[should_panic(expected = "duplicate ActionId")]
fn rejects_duplicate_action_id_between_attached_and_extension_value_object_groups() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [Money],
        services: [],
        commands: [],
        events: [],
        errors: [],
        action_extensions: [DuplicateMoneyExtensionActions],
        query_groups: [],
    };
}
