use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, runtime, validation};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let syntax_fields = input::extract(input)?;
    let fields = crate::field::extract(syntax_fields)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes, &fields)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let descriptor = assembly::assemble(
        &domain_path,
        &input.ident,
        &attributes,
        &fields,
        syntax_fields,
    );
    let runtime = if attributes.runtime {
        runtime::assemble(
            &crate::helper::runtime_api_path::resolve()?,
            &input.ident,
            &attributes,
        )
    } else {
        TokenStream::new()
    };
    Ok(quote::quote! {
        #descriptor
        #runtime
    })
}
