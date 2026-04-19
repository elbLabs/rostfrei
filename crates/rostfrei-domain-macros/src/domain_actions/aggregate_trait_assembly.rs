use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{ItemTrait, Path, WherePredicate};

use super::action::Action;
use super::contract_trait_assembly::{self, Configuration, OutputPolicy};

pub fn assemble(
    domain_path: &Path,
    runtime_path: Option<&Path>,
    mut item: ItemTrait,
    actions: &[Action],
    instance_trait: Option<&syn::Ident>,
) -> syn::Result<TokenStream> {
    if instance_trait.is_some() {
        for action in actions {
            add_raised_event_predicates(domain_path, &mut item, action)?;
        }
    }
    let instance = match (runtime_path, instance_trait) {
        (Some(runtime_path), Some(instance_trait)) => super::aggregate_instance_assembly::assemble(
            domain_path,
            runtime_path,
            &item,
            actions,
            instance_trait,
        ),
        (None, None) => TokenStream::new(),
        _ => unreachable!("runtime path and instance trait are resolved together"),
    };
    if instance_trait.is_some() {
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
            owner_predicate: instance_trait.is_none().then_some(add_root_predicate),
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
    action: &Action,
) -> syn::Result<()> {
    for event in &action.raises {
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
) -> syn::Result<()> {
    let root = action.signature.as_ref().unwrap().root.as_ref().unwrap();
    let predicate: WherePredicate = syn::parse2(
        quote_spanned! {root.span()=> Self: #domain_path::AggregateType<Root = #root>},
    )?;
    item.generics.make_where_clause().predicates.push(predicate);
    Ok(())
}
