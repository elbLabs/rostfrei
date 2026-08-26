use domain::{
    BoundedContext, BoundedContextId, DomainService, DomainServiceDescriptor, DomainServiceId,
    DomainServiceType,
};

#[derive(BoundedContext)]
#[domain(id = "inbox", label = "Inbox")]
struct Inbox;

#[derive(DomainService)]
#[domain(id = "mail-transfer", label = "Mail transfer", context = Inbox)]
struct MailTransfer;

#[test]
fn derives_domain_service_descriptor() {
    assert_eq!(
        MailTransfer::DESCRIPTOR,
        DomainServiceDescriptor {
            id: DomainServiceId {
                context: BoundedContextId("inbox"),
                local: "mail-transfer",
            },
            label: "Mail transfer",
        }
    );
}
