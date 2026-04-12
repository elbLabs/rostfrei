use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainError, DomainErrorDescriptor,
    DomainErrorId, DomainErrorOwnerId, DomainErrorType, DomainIdentity, DomainService,
    DomainServiceId, Entity, EntityId, ValueObject, ValueObjectId,
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
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox", context = Inbox, root = MailboxRoot)]
struct Mailbox;

#[derive(ValueObject)]
#[domain(id = "subject", label = "Subject", owner = Mailbox)]
struct Subject(String);

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer", context = Inbox)]
struct MailTransfer;

#[derive(DomainError)]
#[domain(id = "transfer-denied", label = "Transfer denied", owner = MailTransfer, code = "TRANSFER_DENIED", message = "Mail transfer was denied.")]
struct TransferDenied;

#[derive(DomainError)]
#[domain(id = "mailbox-closed", label = "Mailbox closed", owner = Mailbox, code = "MAILBOX_CLOSED", message = "The mailbox is closed.")]
struct MailboxClosed(String);

#[derive(DomainError)]
#[domain(id = "message-limit", label = "Message limit", owner = MailboxRoot, code = "MESSAGE_LIMIT", message = "The message limit was reached.")]
struct MessageLimit(u64, u64);

#[derive(DomainError)]
#[domain(id = "subject-blank", label = "Subject blank", owner = Subject, code = "SUBJECT_BLANK", message = "The subject must not be blank.")]
struct SubjectBlank {
    supplied: String,
}

fn descriptor(
    owner: DomainErrorOwnerId,
    local: &'static str,
    label: &'static str,
    code: &'static str,
    message: &'static str,
    fields: &'static [domain::FieldDescriptor],
) -> DomainErrorDescriptor {
    DomainErrorDescriptor {
        id: DomainErrorId { owner, local },
        label,
        code,
        message,
        fields,
    }
}

#[test]
fn derives_descriptors_for_each_owner() {
    let context = BoundedContextId("inbox");
    let aggregate = AggregateId {
        context,
        local: "mailbox",
    };
    let entity = EntityId {
        aggregate,
        local: "mailbox-root",
    };
    assert_eq!(
        TransferDenied::DESCRIPTOR,
        descriptor(
            DomainErrorOwnerId::DomainService(DomainServiceId {
                context,
                local: "mail-transfer",
            }),
            "transfer-denied",
            "Transfer denied",
            "TRANSFER_DENIED",
            "Mail transfer was denied.",
            TransferDenied::DESCRIPTOR.fields,
        )
    );
    assert_eq!(
        MailboxClosed::DESCRIPTOR,
        descriptor(
            DomainErrorOwnerId::Aggregate(aggregate),
            "mailbox-closed",
            "Mailbox closed",
            "MAILBOX_CLOSED",
            "The mailbox is closed.",
            MailboxClosed::DESCRIPTOR.fields,
        )
    );
    assert_eq!(
        MessageLimit::DESCRIPTOR,
        descriptor(
            DomainErrorOwnerId::Entity(entity),
            "message-limit",
            "Message limit",
            "MESSAGE_LIMIT",
            "The message limit was reached.",
            MessageLimit::DESCRIPTOR.fields,
        )
    );
    assert_eq!(
        SubjectBlank::DESCRIPTOR,
        descriptor(
            DomainErrorOwnerId::ValueObject(ValueObjectId {
                owner: domain::ValueObjectOwnerId::Aggregate(aggregate),
                local: "subject",
            }),
            "subject-blank",
            "Subject blank",
            "SUBJECT_BLANK",
            "The subject must not be blank.",
            SubjectBlank::DESCRIPTOR.fields,
        )
    );
    assert_eq!(TransferDenied::LOCAL_ID, "transfer-denied");
}

#[test]
fn describes_error_fields() {
    assert_eq!(SubjectBlank::DESCRIPTOR.fields[0].name, "supplied");
    assert_eq!(MessageLimit::DESCRIPTOR.fields.len(), 2);
}

#[test]
fn supports_all_struct_shapes() {
    let _ = TransferDenied;
    let closed = MailboxClosed("archived".to_owned());
    let limit = MessageLimit(10, 10);
    let blank = SubjectBlank {
        supplied: " ".to_owned(),
    };
    let root = MailboxRoot { id: MailboxId(1) };
    let subject = Subject("Hello".to_owned());
    assert_eq!(closed.0, "archived");
    assert_eq!((limit.0, limit.1), (10, 10));
    assert_eq!(blank.supplied, " ");
    assert_eq!(root.id.0, 1);
    assert_eq!(subject.0, "Hello");
}
