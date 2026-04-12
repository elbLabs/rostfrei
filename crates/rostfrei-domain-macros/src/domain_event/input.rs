use syn::{Data, DeriveInput, Fields, Result};

pub fn extract(input: &DeriveInput) -> Result<&Fields> {
    validate_generics(input)?;
    validate_struct(input).map(|data| &data.fields)
}

fn validate_generics(input: &DeriveInput) -> Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            "DomainEvent only supports non-generic structs",
        ))
    }
}

fn validate_struct(input: &DeriveInput) -> Result<&syn::DataStruct> {
    match &input.data {
        Data::Struct(data) => Ok(data),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "DomainEvent only supports non-generic structs",
        )),
    }
}
