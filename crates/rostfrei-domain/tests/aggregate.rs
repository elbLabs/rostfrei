use domain::{
    Aggregate, AggregateDescriptor, AggregateId, AggregateType, BoundedContext, BoundedContextId,
    DomainIdentity, Entity, EntityId,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainIdentity)]
#[domain(owner = MailboxRoot)]
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

#[test]
fn derives_aggregate_descriptor_with_context_id() {
    assert_eq!(
        Mailbox::DESCRIPTOR,
        AggregateDescriptor {
            id: AggregateId {
                context: BoundedContextId("inbox"),
                local: "mailbox",
            },
            label: "Mailbox",
            root: EntityId {
                aggregate: AggregateId {
                    context: BoundedContextId("inbox"),
                    local: "mailbox",
                },
                local: "mailbox-root",
            },
        }
    );
    let root = MailboxRoot { id: MailboxId(1) };
    assert_eq!(root.id.0, 1);
}
