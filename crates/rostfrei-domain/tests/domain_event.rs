use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainEvent, DomainEventDescriptor,
    DomainEventId, DomainEventType, DomainIdentity, Entity,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainIdentity)]
struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox")]
struct MailboxRoot {
    id: MailboxId,
}

impl domain::EntityDefinition for MailboxRoot {
    type Owner = Mailbox;
    type Identity = MailboxId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Aggregate)]
#[domain(id = "mailbox", label = "Mailbox")]
struct Mailbox;

impl domain::AggregateDefinition for Mailbox {
    type Context = Inbox;
    type Root = MailboxRoot;
    type Event = MailboxEvents;
}

#[allow(dead_code)]
#[derive(domain::AggregateEvents)]
enum MailboxEvents {
    Event0(MailboxCreated),
    Event1(MessageReceived),
    Event2(MailboxRenamed),
}

#[derive(DomainEvent)]
#[domain(id = "mailbox-created", label = "Mailbox created")]
struct MailboxCreated;

#[derive(DomainEvent)]
#[domain(
    id = "message-received",
    label = "Message received",
    schema_version = 2
)]
struct MessageReceived(String);

#[derive(DomainEvent)]
#[domain(id = "mailbox-renamed", label = "Mailbox renamed")]
struct MailboxRenamed {
    name: String,
}

#[test]
fn derives_domain_event_descriptor() {
    assert_eq!(
        <MailboxCreated as DomainEventType<Mailbox>>::DESCRIPTOR,
        DomainEventDescriptor {
            id: DomainEventId {
                aggregate: AggregateId {
                    context: BoundedContextId("inbox"),
                    local: "mailbox",
                },
                local: "mailbox-created",
            },
            label: "Mailbox created",
            schema_version: 1,
            fields: &[],
        }
    );
    assert_eq!(<MailboxCreated as DomainEvent>::LOCAL_ID, "mailbox-created");
    assert_eq!(<MailboxCreated as DomainEvent>::SCHEMA_VERSION, 1);
    assert_eq!(<MessageReceived as DomainEvent>::SCHEMA_VERSION, 2);
}

#[test]
fn describes_event_fields() {
    assert_eq!(MailboxRenamed::FIELDS[0].name, "name");
    assert_eq!(
        MailboxRenamed::FIELDS[0].value.kind,
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
rostfrei_domain_macros::__install_test_macro_support!();
