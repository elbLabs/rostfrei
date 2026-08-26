use syn::{Data, DataEnum, DeriveInput, Fields};

pub fn extract(input: &DeriveInput) -> syn::Result<&DataEnum> {
    validate_generics(input)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "EntityLifecycle only supports non-generic fieldless enums",
        ));
    };
    validate_variants(data)?;
    Ok(data)
}

fn validate_generics(input: &DeriveInput) -> syn::Result<()> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "EntityLifecycle only supports non-generic fieldless enums",
        ));
    }
    Ok(())
}

fn validate_variants(data: &DataEnum) -> syn::Result<()> {
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            data.enum_token,
            "EntityLifecycle enums require at least one state",
        ));
    }
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                &variant.fields,
                "EntityLifecycle state variants must be fieldless",
            ));
        }
        if let Some((_, discriminant)) = &variant.discriminant {
            return Err(syn::Error::new_spanned(
                discriminant,
                "EntityLifecycle state variants cannot have explicit discriminants",
            ));
        }
    }
    Ok(())
}
