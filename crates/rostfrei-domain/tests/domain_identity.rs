use domain::{
    Aggregate, AggregateType, BoundedContext, DomainIdentity, DomainIdentityId, Entity, EntityId,
    EntityType, FieldKind,
};

#[derive(BoundedContext)]
#[domain(id = "mail", label = "Mail")]
struct Mail;

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
    type Context = Mail;
    type Root = MailboxRoot;
    type Event = domain::NoDomainEvents;
}

const fn assert_domain_identity<T: DomainIdentity>() {}

#[test]
fn derives_identity_marker_and_entity_scoped_descriptor() {
    assert_domain_identity::<MailboxId>();
    let identity = MailboxId(7);
    assert_eq!(identity.0, 7);
    let root = MailboxRoot { id: MailboxId(8) };
    assert_eq!(root.id.0, 8);
    let descriptor = MailboxRoot::DESCRIPTOR.identity.identity;
    assert_eq!(
        descriptor,
        DomainIdentityId {
            owner: MailboxRoot::DESCRIPTOR.id
        }
    );
    assert_eq!(MailboxRoot::DESCRIPTOR.identity.identity, descriptor);
    assert_eq!(
        MailboxRoot::DESCRIPTOR.fields[0].value.kind,
        FieldKind::DomainIdentity(descriptor)
    );
    assert_eq!(
        descriptor.owner,
        EntityId {
            aggregate: Mailbox::DESCRIPTOR.id,
            local: "mailbox-root"
        }
    );
}
