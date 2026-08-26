use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainEvent, DomainEventDescriptor,
    DomainEventId, DomainEventType, DomainIdentity, Entity,
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

#[derive(DomainEvent)]
#[domain(id = "mailbox-created", label = "Mailbox created", owner = Mailbox)]
struct MailboxCreated;

#[derive(DomainEvent)]
#[domain(id = "message-received", label = "Message received", owner = Mailbox)]
struct MessageReceived(String);

#[derive(DomainEvent)]
#[domain(id = "mailbox-renamed", label = "Mailbox renamed", owner = Mailbox)]
struct MailboxRenamed {
    name: String,
}

#[test]
fn derives_domain_event_descriptor() {
    assert_eq!(
        MailboxCreated::DESCRIPTOR,
        DomainEventDescriptor {
            id: DomainEventId {
                aggregate: AggregateId {
                    context: BoundedContextId("inbox"),
                    local: "mailbox",
                },
                local: "mailbox-created",
            },
            label: "Mailbox created",
            fields: &[],
        }
    );
    assert_eq!(MailboxCreated::LOCAL_ID, "mailbox-created");
}

#[test]
fn describes_event_fields() {
    assert_eq!(MailboxRenamed::DESCRIPTOR.fields[0].name, "name");
    assert_eq!(
        MailboxRenamed::DESCRIPTOR.fields[0].value.kind,
        domain::FieldKind::Scalar(domain::ScalarType::String)
    );
}

#[test]
fn supports_all_struct_shapes() {
    let _ = MailboxCreated;
    let received = MessageReceived("hello".to_owned());
    let renamed = MailboxRenamed {
        name: "Primary".to_owned(),
    };
    let root = MailboxRoot { id: MailboxId(1) };
    assert_eq!(received.0, "hello");
    assert_eq!(renamed.name, "Primary");
    assert_eq!(root.id.0, 1);
}
