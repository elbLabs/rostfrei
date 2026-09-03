use syn::{Data, DeriveInput};

pub fn validate(input: &DeriveInput) -> syn::Result<()> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "ValueObject only supports non-generic structs and enums",
        ));
    }
    if matches!(input.data, Data::Union(_)) {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "ValueObject only supports non-generic structs and enums",
        ));
    }
    match &input.data {
        Data::Struct(data) => reject_field_attributes(data.fields.iter())?,
        Data::Enum(data) => {
            for variant in &data.variants {
                reject_attributes(&variant.attrs)?;
                reject_field_attributes(variant.fields.iter())?;
            }
        }
        Data::Union(_) => {}
    }
    Ok(())
}

fn reject_field_attributes<'a>(fields: impl Iterator<Item = &'a syn::Field>) -> syn::Result<()> {
    for field in fields {
        reject_attributes(&field.attrs)?;
    }
    Ok(())
}

fn reject_attributes(attributes: &[syn::Attribute]) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| crate::helper::domain_attribute::is_helper(attribute))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "ValueObject domain attributes are only supported on the type declaration",
        ));
    }
    Ok(())
}
