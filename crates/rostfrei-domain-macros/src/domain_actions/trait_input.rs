use syn::{ItemTrait, Visibility};

pub fn validate(item: ItemTrait) -> syn::Result<ItemTrait> {
    validate_visibility(&item)?;
    validate_common(&item)?;
    Ok(item)
}

pub fn validate_common(item: &ItemTrait) -> syn::Result<()> {
    validate_kind(item)?;
    validate_generics(item)?;
    validate_inheritance(item)
}

fn validate_visibility(item: &ItemTrait) -> syn::Result<()> {
    if matches!(item.vis, Visibility::Public(_)) {
        return Err(syn::Error::new_spanned(
            &item.vis,
            "domain action contract traits cannot have unrestricted public visibility",
        ));
    }
    Ok(())
}

fn validate_kind(item: &ItemTrait) -> syn::Result<()> {
    if let Some(unsafety) = &item.unsafety {
        return Err(syn::Error::new_spanned(
            unsafety,
            "domain action contract traits cannot be unsafe",
        ));
    }
    if let Some(auto_token) = &item.auto_token {
        return Err(syn::Error::new_spanned(
            auto_token,
            "domain action contract traits cannot be auto traits",
        ));
    }
    Ok(())
}

fn validate_generics(item: &ItemTrait) -> syn::Result<()> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics.params,
            "domain action contract traits cannot be generic",
        ));
    }
    if let Some(where_clause) = &item.generics.where_clause {
        return Err(syn::Error::new_spanned(
            where_clause,
            "domain action contract traits cannot have trait-level where clauses",
        ));
    }
    Ok(())
}

fn validate_inheritance(item: &ItemTrait) -> syn::Result<()> {
    if !item.supertraits.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.supertraits,
            "domain action contract traits cannot have existing supertraits",
        ));
    }
    Ok(())
}
