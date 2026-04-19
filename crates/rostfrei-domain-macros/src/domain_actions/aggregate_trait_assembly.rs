use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Ident, ItemTrait, Path, WherePredicate};

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};
use super::signature::ParsedSignature;

pub fn assemble(
    domain_path: &Path,
    instance: Option<(Path, &Ident)>,
    mut item: ItemTrait,
    actions: &[Action],
) -> syn::Result<TokenStream> {
    let has_instance = instance.is_some();
    if has_instance {
        add_raised_event_predicates(domain_path, &mut item, actions)?;
    }
    let instance = instance.map_or_else(
        || Ok(TokenStream::new()),
        |(runtime_path, instance_trait)| {
            super::aggregate_instance_assembly::assemble(
                domain_path,
                &runtime_path,
                &item,
                actions,
                instance_trait,
            )
        },
    )?;
    if has_instance {
        item.items.clear();
    }
    let contract = contract_trait_assembly::assemble(
        domain_path,
        item,
        actions,
        Configuration {
            owner_supertrait: syn::parse_quote!(#domain_path::AggregateActionOwnerType),
            output_policy: OutputPolicy::Declared(syn::parse_quote!(
                #domain_path::__private::AggregateActionOutput
            )),
            owner_predicate: (!has_instance).then_some(add_root_predicate),
        },
    )?;
    Ok(quote! {
        #contract
        #instance
    })
}

fn add_raised_event_predicates(
    domain_path: &Path,
    item: &mut ItemTrait,
    actions: &[Action],
) -> syn::Result<()> {
    let mut event_keys = HashSet::new();
    for event in actions
        .iter()
        .flat_map(|action| &action.raises)
        .filter(|event| event_keys.insert(event.to_token_stream().to_string()))
    {
        let predicate: WherePredicate = syn::parse2(
            quote_spanned! {event.span()=> #event: #domain_path::DomainEventType<Owner = Self>},
        )?;
        item.generics.make_where_clause().predicates.push(predicate);
    }
    Ok(())
}

fn add_root_predicate(
    domain_path: &Path,
    item: &mut ItemTrait,
    action: &Action,
    signature: &ParsedSignature,
) -> syn::Result<()> {
    let Some(root) = signature.root.as_ref() else {
        return Err(syn::Error::new_spanned(
            &action.syntax.ident,
            "aggregate actions require a first `root: &mut RootType` parameter",
        ));
    };
    let predicate: WherePredicate = syn::parse2(
        quote_spanned! {root.span()=> Self: #domain_path::AggregateType<Root = #root>},
    )?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}
