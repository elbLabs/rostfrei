#![allow(dead_code)]

use domain::{
    Aggregate, AggregateType, BoundedContext, BoundedContextId, DomainIdentity, DomainIdentityType,
    Entity, FieldDescriptor, FieldKind, FieldValue, FieldWrapper, ScalarType, SemanticScalar,
    SemanticScalarDescriptor, ValueObject, ValueObjectDescriptor, ValueObjectId,
    ValueObjectOwnerId, ValueObjectShapeDescriptor, ValueObjectType, ValueObjectVariantDescriptor,
    ValueObjectVariantShapeDescriptor, domain_model,
};
use serde_json::json;

struct ExternalCode;

struct ExternalCodeScalar;

impl SemanticScalar for ExternalCodeScalar {
    type Value = ExternalCode;

    const DESCRIPTOR: SemanticScalarDescriptor = SemanticScalarDescriptor {
        id: "external-code",
        label: "External code",
        representation: ScalarType::String,
    };
}

#[derive(BoundedContext)]
#[domain(id = "tagged-values", label = "Tagged values")]
struct TaggedValues;

#[derive(DomainIdentity)]
#[domain(owner = RecordRoot)]
struct RecordId(u64);

#[derive(Entity)]
#[domain(id = "record-root", label = "Record")]
struct RecordRoot {
    #[domain(identity)]
    id: RecordId,
}

impl domain::EntityDefinition for RecordRoot {
    type Owner = Records;
    type Identity = RecordId;
}

#[derive(Aggregate)]
#[domain(id = "records", label = "Records")]
struct Records;

impl domain::AggregateDefinition for Records {
    type Context = TaggedValues;
    type Root = RecordRoot;
    type Event = domain::NoDomainEvents;
}

#[derive(ValueObject)]
#[domain(id = "nested-value", label = "Nested value", owner = TaggedValues)]
struct NestedValue(String);

#[derive(ValueObject)]
#[domain(id = "mixed-value", label = "Mixed value", owner = TaggedValues)]
enum MixedValue {
    Unit,
    EmptyTuple(),
    EmptyStruct {},
    Scalars(u8, Option<Vec<String>>),
    Fields {
        #[domain(identity)]
        identity: RecordId,
        #[domain(value_object)]
        nested: Vec<Option<NestedValue>>,
        #[domain(aggregate_ref = Records)]
        aggregate: Option<RecordId>,
        #[domain(scalar = ExternalCodeScalar)]
        code: ExternalCode,
    },
}

#[test]
fn describes_mixed_tagged_enum_exactly() {
    assert_eq!(
        MixedValue::DESCRIPTOR,
        ValueObjectDescriptor {
            id: ValueObjectId {
                owner: ValueObjectOwnerId::BoundedContext(BoundedContextId("tagged-values")),
                local: "mixed-value",
            },
            label: "Mixed value",
            shape: ValueObjectShapeDescriptor::TaggedEnum {
                variants: &[
                    ValueObjectVariantDescriptor {
                        name: "Unit",
                        shape: ValueObjectVariantShapeDescriptor::Unit,
                    },
                    ValueObjectVariantDescriptor {
                        name: "EmptyTuple",
                        shape: ValueObjectVariantShapeDescriptor::Tuple { fields: &[] },
                    },
                    ValueObjectVariantDescriptor {
                        name: "EmptyStruct",
                        shape: ValueObjectVariantShapeDescriptor::Struct { fields: &[] },
                    },
                    ValueObjectVariantDescriptor {
                        name: "Scalars",
                        shape: ValueObjectVariantShapeDescriptor::Tuple {
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
                                        kind: FieldKind::Scalar(ScalarType::String),
                                        wrappers: &[FieldWrapper::Optional, FieldWrapper::List],
                                    },
                                },
                            ],
                        },
                    },
                    ValueObjectVariantDescriptor {
                        name: "Fields",
                        shape: ValueObjectVariantShapeDescriptor::Struct {
                            fields: &[
                                FieldDescriptor {
                                    name: "identity",
                                    value: FieldValue {
                                        kind: FieldKind::DomainIdentity(RecordId::DESCRIPTOR.id),
                                        wrappers: &[],
                                    },
                                },
                                FieldDescriptor {
                                    name: "nested",
                                    value: FieldValue {
                                        kind: FieldKind::ValueObject(NestedValue::DESCRIPTOR.id),
                                        wrappers: &[FieldWrapper::List, FieldWrapper::Optional],
                                    },
                                },
                                FieldDescriptor {
                                    name: "aggregate",
                                    value: FieldValue {
                                        kind: FieldKind::AggregateReference(Records::DESCRIPTOR.id),
                                        wrappers: &[FieldWrapper::Optional],
                                    },
                                },
                                FieldDescriptor {
                                    name: "code",
                                    value: FieldValue {
                                        kind: FieldKind::SemanticScalar(
                                            ExternalCodeScalar::DESCRIPTOR,
                                        ),
                                        wrappers: &[],
                                    },
                                },
                            ],
                        },
                    },
                ],
            },
        }
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn projects_tagged_enum_variant_shapes_exactly() {
    let model = domain_model! {
        contexts: [TaggedValues],
        aggregates: [Records],
        entities: [RecordRoot],
        identities: [RecordId],
        value_objects: [NestedValue, MixedValue],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
    .expect("tagged value object model projection should succeed");

    let tagged = &model["valueObjects"][1];
    assert_eq!(
        tagged,
        &json!({
            "id": {
                "owner": { "kind": "boundedContext", "id": "tagged-values" },
                "local": "mixed-value",
            },
            "label": "Mixed value",
            "variants": ["Unit", "EmptyTuple", "EmptyStruct", "Scalars", "Fields"],
            "variantShapes": [{
                "name": "Unit",
                "kind": "unit",
            }, {
                "name": "EmptyTuple",
                "kind": "tuple",
                "fields": [],
            }, {
                "name": "EmptyStruct",
                "kind": "struct",
                "fields": [],
            }, {
                "name": "Scalars",
                "kind": "tuple",
                "fields": [{
                    "name": "0",
                    "value": { "kind": "scalar", "scalar": "u8" },
                }, {
                    "name": "1",
                    "value": {
                        "kind": "optional",
                        "value": {
                            "kind": "list",
                            "element": { "kind": "scalar", "scalar": "string" },
                        },
                    },
                }],
            }, {
                "name": "Fields",
                "kind": "struct",
                "fields": [{
                    "name": "identity",
                    "value": {
                        "kind": "identity",
                        "id": {
                            "owner": {
                                "aggregate": { "context": "tagged-values", "local": "records" },
                                "local": "record-root",
                            },
                        },
                    },
                }, {
                    "name": "nested",
                    "value": {
                        "kind": "list",
                        "element": {
                            "kind": "optional",
                            "value": {
                                "kind": "valueObject",
                                "id": {
                                    "owner": {
                                        "kind": "boundedContext",
                                        "id": "tagged-values",
                                    },
                                    "local": "nested-value",
                                },
                            },
                        },
                    },
                }, {
                    "name": "aggregate",
                    "value": {
                        "kind": "optional",
                        "value": {
                            "kind": "aggregateReference",
                            "aggregate": { "context": "tagged-values", "local": "records" },
                        },
                    },
                }, {
                    "name": "code",
                    "value": {
                        "kind": "scalar",
                        "scalar": {
                            "kind": "semantic",
                            "id": "external-code",
                            "label": "External code",
                            "representation": "string",
                        },
                    },
                }],
            }],
        })
    );
    assert!(tagged.get("fields").is_none());
    assert!(tagged["variantShapes"][0].get("fields").is_none());
}
