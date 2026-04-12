use syn::{ItemTrait, Visibility};

pub fn validate(item: ItemTrait, owner: &str) -> syn::Result<ItemTrait> {
    validate_visibility(&item, owner)?;
    super::trait_input::validate_common(&item)?;
    Ok(item)
}

fn validate_visibility(item: &ItemTrait, owner: &str) -> syn::Result<()> {
    if matches!(item.vis, Visibility::Public(_)) {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        &item.vis,
        format!("{owner} action contract traits require unrestricted public visibility"),
    ))
}
