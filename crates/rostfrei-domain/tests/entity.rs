use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainIdentity, DomainIdentityId,
    Entity, EntityDefinition, EntityDescriptor, EntityId, EntityType, FieldDescriptor, FieldKind,
    FieldValue, ScalarType,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainIdentity)]
struct MailboxId(u64);

#[derive(Entity)]
#[domain(id = "mailbox-root", label = "Mailbox")]
struct MailboxRoot {
    r#id: MailboxId,
    message_count: usize,
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
    type Event = domain::NoDomainEvents;
}

#[test]
fn derives_entity_descriptor_with_identity_metadata() {
    assert_eq!(
        MailboxRoot::DESCRIPTOR,
        EntityDescriptor {
            id: EntityId {
                aggregate: AggregateId {
                    context: BoundedContextId("inbox"),
                    local: "mailbox",
                },
                local: "mailbox-root",
            },
            label: "Mailbox",
            identity: DomainIdentityId {
                owner: MailboxRoot::DESCRIPTOR.id,
            },
            fields: &[
                FieldDescriptor {
                    name: "id",
                    value: FieldValue {
                        kind: FieldKind::Opaque,
                        wrappers: &[],
                    },
                },
                FieldDescriptor {
                    name: "message_count",
                    value: FieldValue {
                        kind: FieldKind::Scalar(ScalarType::Usize),
                        wrappers: &[],
                    },
                },
            ],
        }
    );
    assert_eq!(MailboxRoot::LOCAL_ID, "mailbox-root");
    let root = MailboxRoot {
        r#id: MailboxId(1),
        message_count: 2,
    };
    assert_eq!(root.identity().0, 1);
    assert_eq!(root.r#id.0, 1);
    assert_eq!(root.message_count, 2);
}
