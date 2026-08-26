use syn::{Data, DataEnum, DeriveInput, Fields, Result, Variant as SynVariant};

use super::ir::{Shape, Variant, VariantShape};

pub enum ValidatedInput<'a> {
    Struct(&'a Fields),
    Enum(&'a DataEnum),
}

pub fn validate(input: &DeriveInput) -> Result<ValidatedInput<'_>> {
    validate_generics(input)?;
    match &input.data {
        Data::Struct(data) => Ok(ValidatedInput::Struct(&data.fields)),
        Data::Enum(data) => {
            validate_enum(data)?;
            Ok(ValidatedInput::Enum(data))
        }
        Data::Union(_) => Err(syn::Error::new_spanned(
            &input.ident,
            "ValueObject only supports structs or enums",
        )),
    }
}

pub fn normalize(input: ValidatedInput<'_>) -> Result<Shape> {
    match input {
        ValidatedInput::Struct(fields) => Ok(Shape::Struct {
            fields: crate::field::extract(fields)?,
        }),
        ValidatedInput::Enum(data) => normalize_enum(data),
    }
}

fn normalize_enum(data: &DataEnum) -> Result<Shape> {
    if data
        .variants
        .iter()
        .all(|variant| matches!(&variant.fields, Fields::Unit))
    {
        return Ok(Shape::Enum {
            variants: data
                .variants
                .iter()
                .map(|variant| variant.ident.to_string())
                .collect(),
        });
    }
    Ok(Shape::TaggedEnum {
        variants: data
            .variants
            .iter()
            .map(normalize_variant)
            .collect::<Result<_>>()?,
    })
}

fn normalize_variant(variant: &SynVariant) -> Result<Variant> {
    let shape = match &variant.fields {
        Fields::Unit => VariantShape::Unit,
        fields @ Fields::Unnamed(_) => VariantShape::Tuple {
            fields: crate::field::extract(fields)?,
        },
        fields @ Fields::Named(_) => VariantShape::Struct {
            fields: crate::field::extract(fields)?,
        },
    };
    Ok(Variant {
        name: variant.ident.to_string(),
        shape,
    })
}

fn validate_generics(input: &DeriveInput) -> Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            "ValueObject does not support generics",
        ))
    }
}

fn validate_enum(data: &DataEnum) -> Result<()> {
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            data.enum_token,
            "ValueObject enums must declare at least one variant",
        ));
    }
    for variant in &data.variants {
        if let Some((_, discriminant)) = &variant.discriminant {
            return Err(syn::Error::new_spanned(
                discriminant,
                "ValueObject enum variants cannot have explicit discriminants",
            ));
        }
        if let Some(attribute) = variant
            .attrs
            .iter()
            .find(|attribute| attribute.path().is_ident("domain"))
        {
            return Err(syn::Error::new_spanned(
                attribute,
                "domain attributes are not supported on ValueObject enum variants",
            ));
        }
    }
    Ok(())
}
