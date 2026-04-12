#![allow(dead_code)]

use domain::{
    Aggregate, AggregateType, BoundedContext, DomainIdentity, DomainIdentityType, Entity,
    EntityType, FieldKind, FieldWrapper, ScalarType, ValueObject, ValueObjectType,
};

#[derive(BoundedContext)]
#[domain(id = "catalog", label = "Catalog")]
struct Catalog;

#[derive(DomainIdentity)]
#[domain(owner = ProductRoot)]
struct ProductId(u64);

#[derive(DomainIdentity)]
#[domain(owner = Part)]
struct PartId(u64);

#[derive(ValueObject)]
#[domain(id = "dimensions", label = "Dimensions", owner = Catalog)]
struct Dimensions(u16, u16);

#[derive(ValueObject)]
#[domain(id = "details", label = "Details", owner = Catalog)]
struct Details {
    #[domain(identity)]
    product_id: ProductId,
    r#type: String,
    #[domain(value_object)]
    dimensions: Option<Dimensions>,
    #[domain(aggregate_ref = Product)]
    related: Vec<Option<ProductId>>,
}

#[derive(Entity)]
#[domain(id = "part", label = "Part", owner = Product)]
struct Part {
    #[domain(identity)]
    id: PartId,
}

#[derive(Entity)]
#[domain(id = "product-root", label = "Product", owner = Product)]
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
}

#[derive(Aggregate)]
#[domain(id = "product", label = "Product", context = Catalog, root = ProductRoot)]
struct Product;

#[derive(ValueObject)]
#[domain(id = "scalars", label = "Scalars", owner = Catalog)]
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
#[domain(id = "marker", label = "Marker", owner = Catalog)]
struct Marker;

#[test]
fn describes_entity_roles_wrappers_order_and_raw_names() {
    let fields = ProductRoot::DESCRIPTOR.fields;
    assert_eq!(
        fields.iter().map(|field| field.name).collect::<Vec<_>>(),
        ["id", "active", "parts", "details", "parent"]
    );
    assert_eq!(
        fields[0].value.kind,
        FieldKind::DomainIdentity(ProductId::DESCRIPTOR.id)
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
}

#[test]
fn describes_value_object_shapes_composition_and_all_scalars() {
    let domain::ValueObjectShapeDescriptor::Struct { fields } = Details::DESCRIPTOR.shape else {
        panic!()
    };
    assert_eq!(fields[0].name, "product_id");
    assert_eq!(
        fields[0].value.kind,
        FieldKind::DomainIdentity(ProductId::DESCRIPTOR.id)
    );
    assert_eq!(fields[1].name, "type");
    assert!(
        matches!(fields[2].value.kind, FieldKind::ValueObject(id) if id == Dimensions::DESCRIPTOR.id)
    );
    assert!(
        matches!(fields[3].value.kind, FieldKind::AggregateReference(id) if id == Product::DESCRIPTOR.id)
    );
    assert_eq!(
        match Dimensions::DESCRIPTOR.shape {
            domain::ValueObjectShapeDescriptor::Struct { fields } => fields,
            _ => panic!(),
        }
        .iter()
        .map(|field| field.name)
        .collect::<Vec<_>>(),
        ["0", "1"]
    );
    assert!(matches!(
        Marker::DESCRIPTOR.shape,
        domain::ValueObjectShapeDescriptor::Struct { fields: &[] }
    ));
    assert_eq!(
        match Scalars::DESCRIPTOR.shape {
            domain::ValueObjectShapeDescriptor::Struct { fields } => fields,
            _ => panic!(),
        }
        .iter()
        .map(|field| field.value.kind)
        .collect::<Vec<_>>(),
        [
            FieldKind::Scalar(ScalarType::Bool),
            FieldKind::Scalar(ScalarType::String),
            FieldKind::Scalar(ScalarType::Char),
            FieldKind::Scalar(ScalarType::F32),
            FieldKind::Scalar(ScalarType::F64),
            FieldKind::Scalar(ScalarType::I8),
            FieldKind::Scalar(ScalarType::I16),
            FieldKind::Scalar(ScalarType::I32),
            FieldKind::Scalar(ScalarType::I64),
            FieldKind::Scalar(ScalarType::I128),
            FieldKind::Scalar(ScalarType::Isize),
            FieldKind::Scalar(ScalarType::U8),
            FieldKind::Scalar(ScalarType::U16),
            FieldKind::Scalar(ScalarType::U32),
            FieldKind::Scalar(ScalarType::U64),
            FieldKind::Scalar(ScalarType::U128),
            FieldKind::Scalar(ScalarType::Usize),
        ]
    );
}
