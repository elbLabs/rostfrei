#![allow(dead_code)]

use domain::{
    Aggregate, AggregateType, BoundedContext, DomainIdentity, Entity, EntityType, FieldKind,
    FieldWrapper, ScalarType, ValueObject,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
struct ProductId(u64);

#[derive(DomainIdentity)]
struct PartId(u64);

#[derive(ValueObject)]
#[domain(id = "dimensions", label = "Dimensions")]
struct Dimensions(u16, u16);

#[derive(ValueObject)]
#[domain(id = "details", label = "Details")]
struct Details {
    product_id: ProductId,
    r#type: String,
    dimensions: Option<Dimensions>,
    related: Vec<Option<ProductId>>,
}

struct Metadata;

#[derive(Entity)]
#[domain(id = "part", label = "Part")]
struct Part {
    #[domain(identity)]
    id: PartId,
}

impl domain::EntityDefinition for Part {
    type Owner = Product;
    type Identity = PartId;
}

#[derive(Entity)]
#[domain(id = "product-root", label = "Product")]
struct ProductRoot {
    #[domain(identity)]
    r#id: ProductId,
    active: bool,
    #[domain(entity)]
    parts: Vec<Option<Part>>,
    #[domain(value_object)]
    details: Option<Vec<Details>>,
    #[domain(aggregate_ref = Product)]
    parent: Option<ProductId>,
    metadata: Metadata,
}

impl domain::EntityDefinition for ProductRoot {
    type Owner = Product;
    type Identity = ProductId;
}

#[derive(Aggregate)]
#[domain(id = "product", label = "Product")]
struct Product;

impl domain::AggregateDefinition for Product {
    type Context = Catalog;
    type Root = ProductRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(ValueObject)]
#[domain(id = "scalars", label = "Scalars")]
struct Scalars(
    bool,
    String,
    char,
    f32,
    f64,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
);

#[derive(ValueObject)]
#[domain(id = "marker", label = "Marker")]
struct Marker;

#[test]
fn describes_entity_roles_wrappers_order_and_raw_names() {
    let fields = ProductRoot::DESCRIPTOR.fields;
    assert_eq!(
        fields.iter().map(|field| field.name).collect::<Vec<_>>(),
        ["id", "active", "parts", "details", "parent", "metadata"]
    );
    assert_eq!(
        fields[0].value.kind,
        FieldKind::DomainIdentity(ProductRoot::DESCRIPTOR.identity.identity)
    );
    assert_eq!(fields[1].value.kind, FieldKind::Scalar(ScalarType::Bool));
    assert!(matches!(fields[2].value.kind, FieldKind::Entity(id) if id == Part::DESCRIPTOR.id));
    assert_eq!(
        fields[2].value.wrappers,
        &[FieldWrapper::List, FieldWrapper::Optional]
    );
    assert!(
        matches!(fields[3].value.kind, FieldKind::ValueObject(id) if id == Details::DESCRIPTOR.id)
    );
    assert_eq!(
        fields[3].value.wrappers,
        &[FieldWrapper::Optional, FieldWrapper::List]
    );
    assert!(
        matches!(fields[4].value.kind, FieldKind::AggregateReference(id) if id == Product::DESCRIPTOR.id)
    );
    assert_eq!(fields[5].value.kind, FieldKind::Opaque);
}
