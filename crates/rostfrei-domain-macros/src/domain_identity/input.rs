use syn::{Data, DeriveInput, Result};

pub fn validate(input: &DeriveInput) -> Result<()> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "DomainIdentity only supports non-generic structs and enums",
        ));
    }
    if matches!(input.data, Data::Union(_)) {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "DomainIdentity only supports non-generic structs and enums",
        ));
    }
    Ok(())
}
