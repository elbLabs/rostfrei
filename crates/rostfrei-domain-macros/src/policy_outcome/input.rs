use syn::{Data, DataEnum, DeriveInput};

use super::ir::Outcome;

pub fn validate(input: &DeriveInput) -> syn::Result<&DataEnum> {
    reject_outcome_attributes(&input.attrs)?;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "PolicyOutcome only supports non-generic enums",
        ));
    }
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "PolicyOutcome only supports non-generic enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            data.enum_token,
            "PolicyOutcome enums must declare at least one variant",
        ));
    }
    for variant in &data.variants {
        if let Some((_, discriminant)) = &variant.discriminant {
            return Err(syn::Error::new_spanned(
                discriminant,
                "PolicyOutcome variants cannot have explicit discriminants",
            ));
        }
        for field in &variant.fields {
            reject_outcome_attributes(&field.attrs)?;
        }
    }
    Ok(data)
}

pub fn collect(data: &DataEnum) -> syn::Result<Vec<Outcome>> {
    data.variants
        .iter()
        .map(|variant| {
            let attribute = super::attributes::parse(variant)?;
            Ok(Outcome {
                local_id: attribute.id,
                label: attribute.label,
                cfg_attributes: super::cfg_attributes::collect(&variant.attrs),
            })
        })
        .collect()
}

fn reject_outcome_attributes(attributes: &[syn::Attribute]) -> syn::Result<()> {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| attribute.path().is_ident("outcome"))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "outcome attributes are only supported on enum variants",
        ));
    }
    Ok(())
}
