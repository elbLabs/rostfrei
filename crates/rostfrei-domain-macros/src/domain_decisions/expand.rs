use proc_macro2::TokenStream;

pub fn expand(args: TokenStream, tokens: TokenStream) -> syn::Result<TokenStream> {
    let arguments = super::arguments::parse(args)?;
    let mut input = super::input::parse(tokens)?;
    let decisions = super::decision_collection::collect(&mut input.item.items)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(super::assembly::assemble(
        &domain_path,
        &input.item,
        &input.owner,
        &arguments.group,
        &decisions,
        arguments.owner_kind,
    ))
}
