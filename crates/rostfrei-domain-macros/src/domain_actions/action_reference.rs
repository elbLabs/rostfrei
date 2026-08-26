use std::collections::HashMap;

use proc_macro2::Ident;
use quote::quote_spanned;
use syn::ext::IdentExt;
use syn::{ItemTrait, TraitItem};

use super::action::Action;

struct GeneratedReference<'a> {
    action: &'a Action,
    ident: Ident,
}

pub fn add(item: &mut ItemTrait, actions: &[Action]) -> syn::Result<()> {
    let references = generate(actions);
    validate_generated_collisions(&references)?;
    validate_trait_item_collisions(&item.items, &references)?;
    append(item, references)
}

fn generate(actions: &[Action]) -> Vec<GeneratedReference<'_>> {
    actions
        .iter()
        .map(|action| GeneratedReference {
            ident: crate::helper::action_reference::hidden_from_action_id(&action.id),
            action,
        })
        .collect()
}

fn validate_generated_collisions(references: &[GeneratedReference<'_>]) -> syn::Result<()> {
    let mut generated = HashMap::new();
    for reference in references {
        let name = reference.ident.to_string();
        if let Some(previous) = generated.insert(name.clone(), reference) {
            let mut error = syn::Error::new(
                reference.action.id.span(),
                format!(
                    "generated action reference constant `{name}` conflicts with another generated action reference constant"
                ),
            );
            error.combine(syn::Error::new(
                previous.action.id.span(),
                format!("the first generated action reference constant `{name}` is derived here"),
            ));
            return Err(error);
        }
    }
    Ok(())
}

fn validate_trait_item_collisions(
    items: &[TraitItem],
    references: &[GeneratedReference<'_>],
) -> syn::Result<()> {
    let names: HashMap<_, _> = items
        .iter()
        .filter_map(trait_item_identifier)
        .map(|ident| (normalized(ident), ident))
        .collect();
    for reference in references {
        let name = normalized(&reference.ident);
        if let Some(item_ident) = names.get(&name) {
            let mut error = syn::Error::new(
                reference.action.id.span(),
                format!(
                    "generated action reference constant `{name}` conflicts with trait item `{item_ident}`"
                ),
            );
            error.combine(syn::Error::new(
                item_ident.span(),
                format!("trait item `{item_ident}` is declared here"),
            ));
            return Err(error);
        }
    }
    Ok(())
}

fn trait_item_identifier(item: &TraitItem) -> Option<&Ident> {
    match item {
        TraitItem::Const(item) => Some(&item.ident),
        TraitItem::Fn(item) => Some(&item.sig.ident),
        TraitItem::Type(item) => Some(&item.ident),
        _ => None,
    }
}

fn normalized(ident: &Ident) -> String {
    ident.unraw().to_string()
}

fn append(item: &mut ItemTrait, references: Vec<GeneratedReference<'_>>) -> syn::Result<()> {
    for reference in references {
        let ident = reference.ident;
        let id = &reference.action.id;
        item.items.push(syn::parse2(quote_spanned! {id.span()=>
            #[doc(hidden)]
            #[allow(dead_code)]
            const #ident: ::domain::ActionReference<Self> =
                ::domain::ActionReference::<Self>::__from_local(#id);
        })?);
    }
    Ok(())
}
