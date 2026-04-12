use rostfrei_domain::{
    BoundedContext, BoundedContextDescriptor, BoundedContextId, BoundedContextType,
};

#[derive(BoundedContext)]
#[domain(id = "customer-support", label = "Customer Support")]
struct CustomerSupport;

#[test]
fn derives_bounded_context_descriptor() {
    assert_eq!(
        CustomerSupport::DESCRIPTOR,
        BoundedContextDescriptor {
            id: BoundedContextId("customer-support"),
            label: "Customer Support",
        }
    );
}
