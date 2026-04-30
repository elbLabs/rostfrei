use proc_macro2::TokenStream;
use syn::Item;

use super::{assembly, attributes::Attributes, validation};

pub fn expand(arguments: &TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let item = match syn::parse2(input)? {
        Item::Trait(item) => item,
        item => {
            return Err(syn::Error::new_spanned(
                item,
                "domain_query may only be applied to a trait",
            ));
        }
    };
    let attributes = Attributes::parse(arguments)?;
    validation::validate(&attributes)?;
    validation::validate_trait(&item)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(assembly::assemble(&domain_path, item, &attributes))
}
