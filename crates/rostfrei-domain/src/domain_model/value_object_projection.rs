use serde_json::{Value, json};

use crate::{
    ValueObjectShapeDescriptor, ValueObjectVariantDescriptor, ValueObjectVariantShapeDescriptor,
};

use super::field_projection;

pub(super) fn apply_shape(value: &mut Value, shape: ValueObjectShapeDescriptor) {
    match shape {
        ValueObjectShapeDescriptor::Struct { fields } => {
            value["fields"] = field_projection::fields(fields);
        }
        ValueObjectShapeDescriptor::Enum { variants } => {
            value["variants"] = json!(variants);
        }
        ValueObjectShapeDescriptor::TaggedEnum { variants } => {
            value["variants"] = json!(
                variants
                    .iter()
                    .map(|variant| variant.name)
                    .collect::<Vec<_>>()
            );
            value["variantShapes"] = Value::Array(variants.iter().map(variant_shape).collect());
        }
    }
}

fn variant_shape(descriptor: &ValueObjectVariantDescriptor) -> Value {
    match descriptor.shape {
        ValueObjectVariantShapeDescriptor::Unit => json!({
            "name": descriptor.name,
            "kind": "unit",
        }),
        ValueObjectVariantShapeDescriptor::Tuple { fields } => json!({
            "name": descriptor.name,
            "kind": "tuple",
            "fields": field_projection::fields(fields),
        }),
        ValueObjectVariantShapeDescriptor::Struct { fields } => json!({
            "name": descriptor.name,
            "kind": "struct",
            "fields": field_projection::fields(fields),
        }),
    }
}
