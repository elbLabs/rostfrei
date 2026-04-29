use domain::{
    Aggregate, BoundedContext, DomainError, DomainErrorDescriptor, DomainErrorId, DomainIdentity,
    DomainModelError, Entity, JsonErrorPayload, ValueObject, domain_model,
};
use serde_json::json;

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

#[derive(ValueObject)]
#[domain(id = "subject", label = "Subject")]
struct Subject(String);

#[derive(DomainError)]
#[domain(
    id = "transfer-denied",
    label = "Transfer denied",
    code = "TRANSFER_DENIED",
    message = "Mail transfer was denied."
)]
struct TransferDenied;

#[derive(DomainError)]
#[domain(
    id = "mailbox-closed",
    label = "Mailbox closed",
    code = "MAILBOX_CLOSED",
    message = "The mailbox is closed."
)]
struct MailboxClosed(String);

#[derive(DomainError)]
#[domain(
    id = "message-limit",
    label = "Message limit",
    code = "MESSAGE_LIMIT",
    message = "The message limit was reached."
)]
struct MessageLimit(u64, u64);

#[derive(DomainError)]
#[domain(
    id = "subject-blank",
    label = "Subject blank",
    code = "SUBJECT_BLANK",
    message = "The subject must not be blank."
)]
struct SubjectBlank {
    supplied: String,
}

#[test]
fn generated_json_errors_preserve_canonical_code_and_message() {
    assert_eq!(
        SubjectBlank {
            supplied: " ".to_owned(),
        }
        .encode_json()
        .unwrap(),
        json!({
            "code": "SUBJECT_BLANK",
            "message": "The subject must not be blank.",
            "supplied": " ",
        })
    );
}

const fn descriptor(
    local: &'static str,
    label: &'static str,
    code: &'static str,
    message: &'static str,
    fields: &'static [domain::FieldDescriptor],
) -> DomainErrorDescriptor {
    DomainErrorDescriptor {
        id: DomainErrorId(local),
        label,
        code,
        message,
        fields,
    }
}

#[test]
fn derives_owner_independent_descriptors() {
    assert_eq!(
        TransferDenied::DESCRIPTOR,
        descriptor(
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
            "subject-blank",
            "Subject blank",
            "SUBJECT_BLANK",
            "The subject must not be blank.",
            SubjectBlank::DESCRIPTOR.fields,
        )
    );
    assert_eq!(TransferDenied::LOCAL_ID, "transfer-denied");
}

#[derive(DomainError)]
#[domain(
    id = "transfer-denied",
    label = "Duplicate transfer denied",
    code = "TRANSFER_DENIED_AGAIN",
    message = "Duplicate."
)]
struct DuplicateTransferDenied;

#[test]
fn rejects_duplicate_owner_independent_error_ids() {
    let error = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [],
        errors: [TransferDenied, DuplicateTransferDenied],
        query_groups: [],
    }
    .expect_err("duplicate domain error IDs must be rejected");

    assert_eq!(
        error,
        DomainModelError::DuplicateDomainErrorId {
            id: Box::new(DomainErrorId("transfer-denied")),
        }
    );
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
