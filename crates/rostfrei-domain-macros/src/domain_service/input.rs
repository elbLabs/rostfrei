use syn::{Data, DeriveInput, Fields, Result};

pub fn validate(input: &DeriveInput) -> Result<()> {
    validate_generics(input)?;
    validate_unit_struct(input)
}

fn validate_generics(input: &DeriveInput) -> Result<()> {
    if input.generics.params.is_empty() {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.generics,
            "DomainService only supports non-generic unit structs",
        ))
    }
}

fn validate_unit_struct(input: &DeriveInput) -> Result<()> {
    if matches!(&input.data, Data::Struct(data) if matches!(data.fields, Fields::Unit)) {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            &input.ident,
            "DomainService only supports non-generic unit structs",
        ))
    }
}
