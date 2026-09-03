use serde_json::{Value, json};

use crate::{FieldDescriptor, FieldKind, FieldWrapper, ScalarType, SemanticScalarDescriptor};

use super::id_projection::{aggregate as aggregate_id, entity as entity_id};

pub fn fields(descriptors: &'static [FieldDescriptor]) -> Value {
    descriptors.iter().map(field).collect()
}

fn field(descriptor: &FieldDescriptor) -> Value {
    json!({
        "name": descriptor.name,
        "value": value(descriptor),
    })
}

fn value(descriptor: &FieldDescriptor) -> Value {
    let mut value = match descriptor.value.kind {
        FieldKind::Scalar(scalar_type) => scalar(scalar_type),
        FieldKind::SemanticScalar(descriptor) => semantic_scalar(descriptor),
        FieldKind::Entity(id) => json!({ "kind": "entity", "id": entity_id(id) }),
        FieldKind::AggregateReference(id) => {
            json!({ "kind": "aggregateReference", "aggregate": aggregate_id(id) })
        }
        FieldKind::Opaque => json!({ "kind": "opaque" }),
    };
    for wrapper in descriptor.value.wrappers.iter().rev() {
        value = match wrapper {
            FieldWrapper::List => json!({ "kind": "list", "element": value }),
            FieldWrapper::Optional => json!({ "kind": "optional", "value": value }),
        };
    }
    value
}

pub(super) fn scalar(scalar: ScalarType) -> Value {
    json!({ "kind": "scalar", "scalar": scalar_name(scalar) })
}

pub(super) fn semantic_scalar(descriptor: SemanticScalarDescriptor) -> Value {
    json!({ "kind": "scalar", "scalar": semantic_scalar_value(descriptor) })
}

pub(super) fn semantic_scalar_value(descriptor: SemanticScalarDescriptor) -> Value {
    json!({
        "kind": "semantic",
        "id": descriptor.id,
        "label": descriptor.label,
        "representation": scalar_name(descriptor.representation),
    })
}

pub(super) const fn scalar_name(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::Bool => "bool",
        ScalarType::String => "string",
        ScalarType::Char => "char",
        ScalarType::F32 => "f32",
        ScalarType::F64 => "f64",
        ScalarType::I8 => "i8",
        ScalarType::I16 => "i16",
        ScalarType::I32 => "i32",
        ScalarType::I64 => "i64",
        ScalarType::I128 => "i128",
        ScalarType::Isize => "isize",
        ScalarType::U8 => "u8",
        ScalarType::U16 => "u16",
        ScalarType::U32 => "u32",
        ScalarType::U64 => "u64",
        ScalarType::U128 => "u128",
        ScalarType::Usize => "usize",
    }
}
