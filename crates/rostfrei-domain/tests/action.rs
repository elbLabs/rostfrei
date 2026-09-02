use domain::{
    ActionDescriptor, ActionId, ActionOwnerId, ActionOwnerType, Aggregate, AggregateType,
    BoundedContext, BoundedContextId, DomainError, DomainIdentity, DomainService, DomainServiceId,
    DomainServiceType, Entity, EntityId, EntityType, ValueObject, ValueObjectId,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainIdentity)]
struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox")]
struct MailboxRoot {
    #[domain(identity)]
    id: MailboxId,
    name: String,
    archived: bool,
}

impl domain::EntityDefinition for MailboxRoot {
    type Owner = Mailbox;
    type Identity = MailboxId;
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
#[domain(
    id = "mailbox-denied",
    label = "Mailbox denied",
    code = "MAILBOX_DENIED",
    message = "Mailbox denied."
)]
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
#[domain(id = "mail-transfer", label = "Mail transfer")]
struct MailTransfer;

impl domain::DomainServiceDefinition for MailTransfer {
    type Context = Inbox;
}

#[derive(DomainError)]
#[domain(
    id = "transfer-denied",
    label = "Transfer denied",
    code = "TRANSFER_DENIED",
    message = "Transfer denied."
)]
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
struct MessageId(u64);

#[derive(Entity)]
#[domain(id = "message", label = "Message")]
struct Message {
    #[domain(identity)]
    id: MessageId,
    read: bool,
}

impl domain::EntityDefinition for Message {
    type Owner = Mailbox;
    type Identity = MessageId;
}

#[derive(DomainError)]
#[domain(
    id = "message-denied",
    label = "Message denied",
    code = "MESSAGE_DENIED",
    message = "Message denied."
)]
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
#[domain(id = "counter", label = "Counter")]
struct Counter(u8);

#[test]
fn supports_owner_specific_action_signatures() {
    use contracts::{
        MailTransferActions as _, MailboxArchivalActions as _, MailboxManagementActions as _,
    };
    use restricted_contracts::MessageActions as _;

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
    assert_eq!(Counter(3), Counter(3));
}

#[test]
fn preserves_descriptor_shape_and_source_order() {
    let mailbox_contracts = [
        <Mailbox as contracts::MailboxManagementActions>::__DOMAIN_ACTIONS,
        <Mailbox as contracts::MailboxArchivalActions>::__DOMAIN_ACTIONS,
    ];
    let transfer_contract = <MailTransfer as contracts::MailTransferActions>::__DOMAIN_ACTIONS;
    let message_contracts = [<Message as restricted_contracts::MessageActions>::__DOMAIN_ACTIONS];

    assert_eq!(mailbox_contracts.len(), 2);
    assert_eq!(transfer_contract.len(), 2);
    assert_eq!(message_contracts.len(), 1);
    assert_eq!(mailbox_contracts[0].len(), 1);
    assert_eq!(mailbox_contracts[0][0].id.local, "rename");
    assert_eq!(mailbox_contracts[1][0].id.local, "archive");
    assert_eq!(transfer_contract[0].id.local, "available");
    assert_eq!(transfer_contract[1].id.local, "transfer");
    assert_eq!(message_contracts[0].len(), 2);
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
    assert_eq!(Counter::DESCRIPTOR.id, ValueObjectId("counter"));
}
