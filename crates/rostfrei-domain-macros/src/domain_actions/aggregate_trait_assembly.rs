use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::spanned::Spanned;
use syn::{ItemTrait, Path, WherePredicate};

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};

pub fn assemble(
    domain_path: &Path,
    item: ItemTrait,
    actions: &[Action],
) -> syn::Result<TokenStream> {
    contract_trait_assembly::assemble(
        domain_path,
        item,
        actions,
        Configuration {
            owner_supertrait: syn::parse_quote!(#domain_path::AggregateActionOwnerType),
            output_policy: OutputPolicy::Declared(syn::parse_quote!(
                #domain_path::__private::AggregateActionOutput
            )),
            owner_predicate: Some(add_root_predicate),
        },
    )
}

fn add_root_predicate(
    domain_path: &Path,
    item: &mut ItemTrait,
    action: &Action,
) -> syn::Result<()> {
    let root = action.signature.as_ref().unwrap().root.as_ref().unwrap();
    let predicate: WherePredicate = syn::parse2(
        quote_spanned! {root.span()=> Self: #domain_path::AggregateType<Root = #root>},
    )?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}
