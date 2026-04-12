use std::collections::HashMap;

use proc_macro2::Ident;
use quote::quote_spanned;
use syn::ext::IdentExt;
use syn::{ItemTrait, TraitItem};

use super::decision::Decision;

struct GeneratedReference<'a> {
    decision: &'a Decision,
    ident: Ident,
}

pub fn add(item: &mut ItemTrait, decisions: &[Decision]) -> syn::Result<()> {
    let references = generate(decisions);
    validate_generated_collisions(&references)?;
    validate_trait_item_collisions(&item.items, &references)?;
    append(item, references)
}

fn generate(decisions: &[Decision]) -> Vec<GeneratedReference<'_>> {
    decisions
        .iter()
        .map(|decision| GeneratedReference {
            ident: super::decision_reference_name::hidden_from_decision_id(&decision.id),
            decision,
        })
        .collect()
}

fn validate_generated_collisions(references: &[GeneratedReference<'_>]) -> syn::Result<()> {
    let mut generated = HashMap::new();
    for reference in references {
        let name = reference.ident.to_string();
        if let Some(previous) = generated.insert(name.clone(), reference) {
            let mut error = syn::Error::new(
                reference.decision.id.span(),
                format!(
                    "generated decision reference constant `{name}` conflicts with another generated decision reference constant"
                ),
            );
            error.combine(syn::Error::new(
                previous.decision.id.span(),
                format!("the first generated decision reference constant `{name}` is derived here"),
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
                reference.decision.id.span(),
                format!(
                    "generated decision reference constant `{name}` conflicts with trait item `{item_ident}`"
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
        let id = &reference.decision.id;
        item.items.push(syn::parse2(quote_spanned! {id.span()=>
            #[doc(hidden)]
            #[allow(dead_code)]
            const #ident: ::domain::DecisionReference<Self> =
                ::domain::DecisionReference::<Self>::__from_local(#id);
        })?);
    }
    Ok(())
}
