use domain::{
    Aggregate, AggregateId, BoundedContext, BoundedContextId, DomainIdentity, Entity, EntityId,
    EntityType, FieldDescriptor, FieldKind, FieldValue, FieldWrapper, ScalarType, ValueObject,
    ValueObjectDescriptor, ValueObjectId, ValueObjectOwnerId, ValueObjectOwnerType,
    ValueObjectShapeDescriptor, ValueObjectType, domain_actions, domain_model,
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

#[derive(ValueObject)]
#[domain(id = "system-address", label = "System address", owner = Inbox)]
struct SystemAddress;

#[derive(ValueObject)]
#[domain(id = "subject", label = "Subject", owner = Mailbox)]
struct Subject(String);

#[derive(ValueObject)]
#[domain(id = "delivery-window", label = "Delivery window", owner = MailboxRoot)]
struct DeliveryWindow(u8, u8);

#[derive(ValueObject)]
#[domain(id = "sender", label = "Sender", owner = Inbox)]
struct Sender {
    local: String,
    domain: String,
}

#[derive(ValueObject, Clone, Copy, Debug, Default, PartialEq)]
#[domain(
    id = "category-kind",
    label = "Category kind",
    owner = Inbox,
    actions = [CategoryKindActions]
)]
#[repr(u8)]
enum CategoryKind {
    #[default]
    Service,
    Resource,
}

#[derive(ValueObject)]
#[domain(id = "category-selection", label = "Category selection", owner = Inbox)]
struct CategorySelection {
    #[domain(value_object)]
    kinds: Option<Vec<CategoryKind>>,
}

#[derive(DomainIdentity)]
#[domain(owner = CatalogEntry)]
struct CatalogEntryId(u64);

#[derive(Entity)]
#[domain(id = "catalog-entry", label = "Catalog entry", owner = Mailbox)]
struct CatalogEntry {
    #[domain(identity)]
    id: CatalogEntryId,
    #[domain(value_object)]
    kinds: Vec<Option<CategoryKind>>,
}

#[domain_actions(value_object)]
trait CategoryKindActions {
    #[action(id = "from-service-flag", label = "From service flag")]
    fn from_service_flag(input: bool) -> Self;

    #[action(id = "toggle", label = "Toggle category kind")]
    fn toggle(self) -> Self;
}

impl CategoryKindActions for CategoryKind {
    fn from_service_flag(input: bool) -> Self {
        if input { Self::Service } else { Self::Resource }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Service => Self::Resource,
            Self::Resource => Self::Service,
        }
    }
}

const fn owner_id<T: ValueObjectOwnerType>() -> ValueObjectOwnerId {
    T::VALUE_OBJECT_OWNER_ID
}

#[test]
fn derives_descriptors_for_each_owner() {
    let aggregate_id = AggregateId {
        context: BoundedContextId("inbox"),
        local: "mailbox",
    };
    let entity_id = EntityId {
        aggregate: aggregate_id,
        local: "mailbox-root",
    };
    assert_eq!(
        SystemAddress::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId {
                owner: ValueObjectOwnerId::BoundedContext(BoundedContextId("inbox")),
                local: "system-address",
            },
            label: "System address",
            shape: ValueObjectShapeDescriptor::Struct { fields: &[] },
        }
    );
    assert_eq!(
        Subject::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId {
                owner: ValueObjectOwnerId::Aggregate(aggregate_id),
                local: "subject",
            },
            label: "Subject",
            shape: ValueObjectShapeDescriptor::Struct {
                fields: &[FieldDescriptor {
                    name: "0",
                    value: FieldValue {
                        kind: FieldKind::Scalar(ScalarType::String),
                        wrappers: &[],
                    },
                }]
            },
        }
    );
    assert_eq!(
        DeliveryWindow::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId {
                owner: ValueObjectOwnerId::Entity(entity_id),
                local: "delivery-window",
            },
            label: "Delivery window",
            shape: ValueObjectShapeDescriptor::Struct {
                fields: &[
                    FieldDescriptor {
                        name: "0",
                        value: FieldValue {
                            kind: FieldKind::Scalar(ScalarType::U8),
                            wrappers: &[],
                        },
                    },
                    FieldDescriptor {
                        name: "1",
                        value: FieldValue {
                            kind: FieldKind::Scalar(ScalarType::U8),
                            wrappers: &[],
                        },
                    },
                ]
            },
        }
    );
    assert_eq!(owner_id::<Inbox>(), SystemAddress::DESCRIPTOR.id.owner);
    assert_eq!(owner_id::<Mailbox>(), Subject::DESCRIPTOR.id.owner);
    assert_eq!(
        owner_id::<MailboxRoot>(),
        DeliveryWindow::DESCRIPTOR.id.owner
    );
}

#[test]
fn supports_all_struct_shapes() {
    let _ = SystemAddress;
    let subject = Subject("Hello".to_owned());
    let window = DeliveryWindow(9, 17);
    let sender = Sender {
        local: "team".to_owned(),
        domain: "example.com".to_owned(),
    };
    let mailbox = MailboxRoot { id: MailboxId(1) };
    assert_eq!(subject.0, "Hello");
    assert_eq!((window.0, window.1), (9, 17));
    assert_eq!(
        (sender.local, sender.domain),
        ("team".to_owned(), "example.com".to_owned())
    );
    assert_eq!(mailbox.id.0, 1);
}

#[test]
fn derives_fieldless_enum_descriptor_and_supports_actions() {
    assert_eq!(
        CategoryKind::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId {
                owner: ValueObjectOwnerId::BoundedContext(BoundedContextId("inbox")),
                local: "category-kind",
            },
            label: "Category kind",
            shape: ValueObjectShapeDescriptor::Enum {
                variants: &["Service", "Resource"]
            },
        }
    );
    assert_eq!(
        CategoryKind::from_service_flag(true).toggle(),
        CategoryKind::Resource
    );
}

#[test]
fn enum_value_objects_compose_in_fields_and_project_to_json() {
    let selection_value = CategorySelection {
        kinds: Some(vec![CategoryKind::Resource]),
    };
    assert_eq!(selection_value.kinds, Some(vec![CategoryKind::Resource]));

    let ValueObjectShapeDescriptor::Struct { fields } = CategorySelection::DESCRIPTOR.shape else {
        panic!()
    };
    let selection = fields[0];
    assert!(
        matches!(selection.value.kind, FieldKind::ValueObject(id) if id == CategoryKind::DESCRIPTOR.id)
    );
    assert_eq!(
        selection.value.wrappers,
        &[FieldWrapper::Optional, FieldWrapper::List]
    );

    let entity = CatalogEntry::DESCRIPTOR.fields[1];
    assert!(
        matches!(entity.value.kind, FieldKind::ValueObject(id) if id == CategoryKind::DESCRIPTOR.id)
    );
    assert_eq!(
        entity.value.wrappers,
        &[FieldWrapper::List, FieldWrapper::Optional]
    );

    let model = domain_model! {
        contexts: [Inbox],
        aggregates: [Mailbox],
        entities: [MailboxRoot, CatalogEntry],
        identities: [MailboxId, CatalogEntryId],
        value_objects: [CategoryKind, CategorySelection],
        services: [],
        commands: [],
        errors: [],

        query_groups: [],
    };
    let category_kind = &model["valueObjects"][0];
    assert_eq!(category_kind["id"]["local"], "category-kind");
    assert_eq!(
        category_kind["variants"],
        serde_json::json!(["Service", "Resource"])
    );
    assert!(category_kind.get("fields").is_none());
    assert!(category_kind.get("variantShapes").is_none());
    assert!(model["valueObjects"][1].get("variants").is_none());
    assert_eq!(
        model["entities"][1]["fields"][1]["value"]["element"]["value"]["id"]["local"],
        "category-kind"
    );

    let entry = CatalogEntry {
        id: CatalogEntryId(1),
        kinds: vec![Some(CategoryKind::Service)],
    };
    assert_eq!(entry.id.0, 1);
    assert_eq!(entry.kinds, [Some(CategoryKind::Service)]);
}
