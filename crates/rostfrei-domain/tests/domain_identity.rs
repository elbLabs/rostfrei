use rostfrei_domain::{
    Aggregate, AggregateType, BoundedContext, DomainIdentity, DomainIdentityId, DomainIdentityType,
    Entity, EntityId, EntityType, FieldKind, ScalarType,
};

#[derive(BoundedContext)]
#[domain(id = "mail", label = "Mail")]
struct Mail;

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
#[domain(id = "mailbox", label = "Mailbox", context = Mail, root = MailboxRoot)]
struct Mailbox;

fn assert_domain_identity<T: DomainIdentityType>() {}

#[test]
fn derives_domain_identity_type() {
    assert_domain_identity::<MailboxId>();
    let identity = MailboxId(7);
    assert_eq!(identity.0, 7);
    let root = MailboxRoot { id: MailboxId(8) };
    assert_eq!(root.id.0, 8);
    assert_eq!(MailboxId::DESCRIPTOR.scalar, ScalarType::U64);
    assert_eq!(
        MailboxId::DESCRIPTOR.id,
        DomainIdentityId {
            owner: MailboxRoot::DESCRIPTOR.id
        }
    );
    assert_eq!(
        MailboxRoot::DESCRIPTOR.identity.identity,
        MailboxId::DESCRIPTOR.id
    );
    assert_eq!(
        MailboxRoot::DESCRIPTOR.fields[0].value.kind,
        FieldKind::DomainIdentity(MailboxId::DESCRIPTOR.id)
    );
    assert_eq!(
        MailboxId::DESCRIPTOR.id.owner,
        EntityId {
            aggregate: Mailbox::DESCRIPTOR.id,
            local: "mailbox-root"
        }
    );
}
