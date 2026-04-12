use proc_macro2::TokenStream;
use quote::quote_spanned;
use syn::spanned::Spanned;
use syn::{ItemTrait, WherePredicate};

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};

pub fn assemble(item: ItemTrait, actions: &[Action]) -> syn::Result<TokenStream> {
    contract_trait_assembly::assemble(
        item,
        actions,
        Configuration {
            owner_supertrait: syn::parse_quote!(::rostfrei_domain::AggregateActionOwnerType),
            output_policy: OutputPolicy::Declared(syn::parse_quote!(
                ::rostfrei_domain::__private::AggregateActionOutput
            )),
            owner_predicate: Some(add_root_predicate),
        },
    )
}

fn add_root_predicate(item: &mut ItemTrait, action: &Action) -> syn::Result<()> {
    let root = action.signature.as_ref().unwrap().root.as_ref().unwrap();
    let predicate: WherePredicate = syn::parse2(
        quote_spanned! {root.span()=> Self: ::rostfrei_domain::AggregateType<Root = #root>},
    )?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}
