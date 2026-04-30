use domain::{ValueObject, ValueObjectDescriptor, ValueObjectId, domain_model};

#[derive(ValueObject)]
#[domain(id = "subject", label = "Subject")]
struct Subject(String);

#[derive(ValueObject)]
#[domain(id = "delivery-state", label = "Delivery state")]
enum DeliveryState {
    Pending,
    Delivered,
}

#[test]
fn projects_only_id_and_label() {
    let model = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [Subject, DeliveryState],
        services: [],
        errors: [],
    }
    .expect("minimal value objects should project");

    assert_eq!(model["valueObjects"][0]["id"], "subject");
    assert_eq!(model["valueObjects"][0]["label"], "Subject");
    assert!(model["valueObjects"][0].get("fields").is_none());
    assert!(model["valueObjects"][0].get("variants").is_none());
}

const fn assert_value_object<T: ValueObject>() {}

#[test]
fn derives_owner_independent_semantic_descriptors() {
    assert_value_object::<Subject>();
    assert_value_object::<DeliveryState>();
    assert_eq!(
        Subject::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId("subject"),
            label: "Subject",
        }
    );
    assert_eq!(
        DeliveryState::DESCRIPTOR.id,
        ValueObjectId("delivery-state")
    );

    let subject = Subject("Hello".to_owned());
    assert_eq!(subject.0, "Hello");
    let _ = DeliveryState::Pending;
    let _ = DeliveryState::Delivered;
}
