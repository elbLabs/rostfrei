use proc_macro2::TokenStream;
use syn::{ItemImpl, PathArguments, Type, TypePath};

pub struct Input {
    pub item: ItemImpl,
    pub owner: TypePath,
}

pub fn parse(tokens: TokenStream) -> syn::Result<Input> {
    let item = syn::parse2::<ItemImpl>(tokens)?;
    if item.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item,
            "domain_queries only supports inherent impl blocks",
        ));
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "domain_queries does not support generic impl blocks or impl where clauses",
        ));
    }
    let Type::Path(owner) = item.self_ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &item.self_ty,
            "domain_queries requires a concrete path self type",
        ));
    };
    if owner.qself.is_some()
        || owner
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return Err(syn::Error::new_spanned(
            owner,
            "domain_queries requires a concrete path self type",
        ));
    }
    let owner = owner.clone();
    Ok(Input { item, owner })
}
