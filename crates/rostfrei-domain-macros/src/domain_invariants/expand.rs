use proc_macro2::TokenStream;
use syn::{Item, ItemTrait};

pub fn expand(args: TokenStream, tokens: TokenStream) -> syn::Result<TokenStream> {
    match syn::parse2(tokens)? {
        Item::Trait(item) => expand_trait(args, item),
        item => Err(syn::Error::new_spanned(
            item,
            "domain_invariants may only be applied to a trait",
        )),
    }
}

fn expand_trait(args: TokenStream, mut item: ItemTrait) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "domain_invariants does not accept arguments",
        ));
    }
    let invariants = super::invariant_collection::collect(&mut item.items)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    super::assembly::assemble(&domain_path, item, &invariants)
}
