use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, ActionOwnerType, Aggregate, AggregateType,
    BoundedContext, BoundedContextId, DomainError, DomainIdentity, DomainService, DomainServiceId,
    DomainServiceType, Entity, EntityId, EntityType, ValueObject, ValueObjectId,
    ValueObjectOwnerId, ValueObjectType,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainIdentity)]
#[domain(owner = MailboxRoot)]
struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox", owner = Mailbox)]
struct MailboxRoot {
    #[domain(identity)]
    id: MailboxId,
    name: String,
    archived: bool,
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox;

impl domain::AggregateDefinition for Mailbox {
    type Context = Inbox;
    type Root = MailboxRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(DomainError)]
#[domain(id = "mailbox-denied", label = "Mailbox denied", owner = Mailbox, code = "MAILBOX_DENIED", message = "Mailbox denied.")]
struct MailboxDenied;

mod contracts {
    use domain::domain_actions;

    #[domain_actions(aggregate)]
    pub trait MailboxManagementActions {
        #[action(id = "rename", label = "Rename mailbox")]
        fn rename(root: &mut super::MailboxRoot, input: String);
    }

    #[domain_actions(aggregate)]
    pub trait MailboxArchivalActions {
        #[action(id = "archive", label = "Archive mailbox")]
        fn archive(
            root: &mut super::MailboxRoot,
        ) -> core::result::Result<bool, super::MailboxDenied>;
    }

    #[domain_actions(domain_service)]
    pub trait MailTransferActions {
        #[action(id = "available", label = "Transfer available")]
        fn available() -> bool;

        #[action(id = "transfer", label = "Transfer mail")]
        fn transfer(input: u8) -> ::std::result::Result<u8, super::TransferDenied>;
    }
}

pub mod restricted_contracts {
    use domain::domain_actions;

    #[domain_actions(entity)]
    pub(super) trait MessageActions {
        #[action(id = "is-read", label = "Is read")]
        fn is_read(&self) -> bool;

        #[action(id = "set-read", label = "Set read")]
        fn set_read(&mut self, input: bool) -> Result<(), super::MessageDenied>;
    }

    #[domain_actions(value_object)]
    pub(super) trait CounterActions {
        #[action(id = "new", label = "New counter")]
        fn new(input: u8) -> Self;

        #[action(id = "increment", label = "Increment counter")]
        fn increment(self, input: u8) -> std::result::Result<Self, super::CounterDenied>;
    }
}

impl contracts::MailboxManagementActions for Mailbox {
    fn rename(root: &mut MailboxRoot, input: String) {
        root.name = input;
    }
}

impl Mailbox {
    #[must_use]
    const fn unchanged(value: usize) -> usize {
        value.saturating_add(1)
    }
}

impl contracts::MailboxArchivalActions for Mailbox {
    fn archive(root: &mut MailboxRoot) -> core::result::Result<bool, MailboxDenied> {
        root.archived = true;
        Ok(root.archived)
    }
}

#[derive(DomainService)]
#[domain(
    id = "mail-transfer",
    label = "Mail transfer",
    context = Inbox,
    actions = [contracts::MailTransferActions]
)]
struct MailTransfer;

#[derive(DomainError)]
#[domain(id = "transfer-denied", label = "Transfer denied", owner = MailTransfer, code = "TRANSFER_DENIED", message = "Transfer denied.")]
struct TransferDenied;

impl contracts::MailTransferActions for MailTransfer {
    fn available() -> bool {
        true
    }

    fn transfer(input: u8) -> ::std::result::Result<u8, TransferDenied> {
        Ok(input)
    }
}

#[derive(DomainIdentity)]
#[domain(owner = Message)]
struct MessageId(u64);

#[derive(Entity)]
#[domain(
    id = "message",
    label = "Message",
    owner = Mailbox,
    actions = [restricted_contracts::MessageActions]
)]
struct Message {
    #[domain(identity)]
    id: MessageId,
    read: bool,
}

#[derive(DomainError)]
#[domain(id = "message-denied", label = "Message denied", owner = Message, code = "MESSAGE_DENIED", message = "Message denied.")]
struct MessageDenied;

impl restricted_contracts::MessageActions for Message {
    fn is_read(&self) -> bool {
        self.read
    }

    fn set_read(&mut self, input: bool) -> Result<(), MessageDenied> {
        self.read = input;
        Ok(())
    }
}

#[derive(ValueObject, Debug, PartialEq)]
#[domain(
    id = "counter",
    label = "Counter",
    owner = Mailbox,
    actions = [restricted_contracts::CounterActions]
)]
struct Counter(u8);

#[derive(DomainError)]
#[domain(id = "counter-denied", label = "Counter denied", owner = Counter, code = "COUNTER_DENIED", message = "Counter denied.")]
struct CounterDenied;

impl restricted_contracts::CounterActions for Counter {
    fn new(input: u8) -> Self {
        Self(input)
    }

    fn increment(self, input: u8) -> std::result::Result<Self, CounterDenied> {
        self.0.checked_add(input).map(Self).ok_or(CounterDenied)
    }
}

#[test]
fn supports_owner_specific_action_signatures() {
    use contracts::{
        MailTransferActions as _, MailboxArchivalActions as _, MailboxManagementActions as _,
    };
    use restricted_contracts::{CounterActions as _, MessageActions as _};

    let mut root = MailboxRoot {
        id: MailboxId(1),
        name: "Inbox".to_owned(),
        archived: false,
    };
    Mailbox::rename(&mut root, "Work".to_owned());
    assert!(matches!(Mailbox::archive(&mut root), Ok(true)));
    assert_eq!(root.name, "Work");
    assert_eq!(root.id.0, 1);
    assert_eq!(Mailbox::unchanged(2), 3);
    assert!(MailTransfer::available());
    assert!(matches!(MailTransfer::transfer(7), Ok(7)));
    let mut message = Message {
        id: MessageId(2),
        read: false,
    };
    assert!(!message.is_read());
    assert!(matches!(message.set_read(true), Ok(())));
    assert_eq!(message.id.0, 2);
    assert!(matches!(Counter::new(1).increment(2), Ok(Counter(3))));
}

#[test]
fn preserves_descriptor_shape_and_source_order() {
    let mailbox_contracts = [
        <Mailbox as contracts::MailboxManagementActions>::__DOMAIN_ACTIONS,
        <Mailbox as contracts::MailboxArchivalActions>::__DOMAIN_ACTIONS,
    ];
    let transfer_contracts = <MailTransfer as DomainServiceType>::ACTION_CONTRACTS;
    let message_contracts = <Message as EntityType>::ACTION_CONTRACTS;
    let counter_contracts = <Counter as ValueObjectType>::ACTION_CONTRACTS;

    assert_eq!(mailbox_contracts.len(), 2);
    assert_eq!(transfer_contracts.len(), 1);
    assert_eq!(message_contracts.len(), 1);
    assert_eq!(counter_contracts.len(), 1);
    assert_eq!(mailbox_contracts[0].len(), 1);
    assert_eq!(mailbox_contracts[0][0].id.local, "rename");
    assert_eq!(mailbox_contracts[1][0].id.local, "archive");
    assert_eq!(transfer_contracts[0][0].id.local, "available");
    assert_eq!(transfer_contracts[0][1].id.local, "transfer");
    assert_eq!(message_contracts[0].len(), 2);
    assert_eq!(counter_contracts[0].len(), 2);
    assert_eq!(
        mailbox_contracts[0][0],
        ActionDescriptor {
            id: ActionId {
                owner: ActionOwnerId::Aggregate(domain::AggregateId {
                    context: BoundedContextId("inbox"),
                    local: "mailbox",
                }),
                local: "rename",
            },
            label: "Rename mailbox",
            input: Some(domain::ActionInputDescriptor::Scalar(
                domain::ScalarType::String,
            )),
            output: None,
            raises: &[],
            error: None,
        }
    );
}

#[test]
fn uses_the_owner_descriptor_id_for_each_owner_kind() {
    assert_eq!(
        Mailbox::ACTION_OWNER_ID,
        ActionOwnerId::Aggregate(Mailbox::DESCRIPTOR.id)
    );
    assert_eq!(
        MailTransfer::ACTION_OWNER_ID,
        ActionOwnerId::DomainService(MailTransfer::DESCRIPTOR.id)
    );
    assert_eq!(
        Message::ACTION_OWNER_ID,
        ActionOwnerId::Entity(Message::DESCRIPTOR.id)
    );
    assert_eq!(
        Counter::ACTION_OWNER_ID,
        ActionOwnerId::ValueObject(Counter::DESCRIPTOR.id)
    );
    assert_eq!(
        MailTransfer::DESCRIPTOR.id,
        DomainServiceId {
            context: BoundedContextId("inbox"),
            local: "mail-transfer"
        }
    );
    assert_eq!(
        Message::DESCRIPTOR.id,
        EntityId {
            aggregate: Mailbox::DESCRIPTOR.id,
            local: "message"
        }
    );
    assert_eq!(
        Counter::DESCRIPTOR.id,
        ValueObjectId {
            owner: ValueObjectOwnerId::Aggregate(Mailbox::DESCRIPTOR.id),
            local: "counter"
        }
    );
}
