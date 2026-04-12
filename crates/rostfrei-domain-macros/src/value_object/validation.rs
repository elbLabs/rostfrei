use syn::Result;

use crate::field::{Field, Role};

use super::attributes::Attributes;
use super::ir::{Shape, VariantShape};

pub fn validate(attributes: &Attributes, shape: &Shape) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    match shape {
        Shape::Struct { fields } => validate_fields(fields),
        Shape::Enum { .. } => Ok(()),
        Shape::TaggedEnum { variants } => {
            for variant in variants {
                match &variant.shape {
                    VariantShape::Unit => {}
                    VariantShape::Tuple { fields } | VariantShape::Struct { fields } => {
                        validate_fields(fields)?;
                    }
                }
            }
            Ok(())
        }
    }
}

fn validate_fields(fields: &[Field]) -> Result<()> {
    for field in fields {
        if matches!(field.role, Role::Entity) {
            return Err(syn::Error::new_spanned(
                &field.base,
                "contained Entity fields are only valid on Entity",
            ));
        }
    }
    Ok(())
}
