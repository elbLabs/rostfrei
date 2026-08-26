use syn::{Data, DeriveInput, Fields, Result};

pub fn extract(input: &DeriveInput) -> Result<&Fields> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "DomainCommand only supports non-generic structs",
        ));
    }
    match &input.data {
        Data::Struct(data) => Ok(&data.fields),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "DomainCommand only supports non-generic structs",
        )),
    }
}
