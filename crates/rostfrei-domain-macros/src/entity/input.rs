use syn::{Data, DeriveInput, Fields, Result};

pub fn extract(input: &DeriveInput) -> Result<&Fields> {
    validate_generics(input)?;
    extract_named_fields(input)
}

fn validate_generics(input: &DeriveInput) -> Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            "Entity only supports non-generic named-field structs",
        ))
    }
}

fn extract_named_fields(input: &DeriveInput) -> Result<&Fields> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            fields @ Fields::Named(_) => Ok(fields),
            _ => Err(syn::Error::new_spanned(
                &input.ident,
                "Entity only supports non-generic named-field structs",
            )),
        },
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "Entity only supports non-generic named-field structs",
        )),
    }
}
