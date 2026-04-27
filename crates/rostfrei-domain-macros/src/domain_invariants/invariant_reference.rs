use std::collections::HashMap;

use proc_macro2::Ident;
use quote::quote_spanned;
use syn::ext::IdentExt;
use syn::{ItemTrait, Path, TraitItem};

use super::invariant::Invariant;

struct GeneratedReference<'a> {
    invariant: &'a Invariant,
    ident: Ident,
}

pub fn add(domain_path: &Path, item: &mut ItemTrait, invariants: &[Invariant]) -> syn::Result<()> {
    let references = generate(invariants);
    validate_generated_collisions(&references)?;
    validate_trait_item_collisions(&item.items, &references)?;
    append(domain_path, item, references)
}

fn generate(invariants: &[Invariant]) -> Vec<GeneratedReference<'_>> {
    invariants
        .iter()
        .map(|invariant| GeneratedReference {
            ident: super::invariant_reference_name::hidden_from_invariant_id(&invariant.id),
            invariant,
        })
        .collect()
}

fn validate_generated_collisions(references: &[GeneratedReference<'_>]) -> syn::Result<()> {
    let mut generated = HashMap::new();
    for reference in references {
        let name = reference.ident.to_string();
        if let Some(previous) = generated.insert(name.clone(), reference) {
            let mut error = syn::Error::new(
                reference.invariant.id.span(),
                format!(
                    "generated invariant reference constant `{name}` conflicts with another generated invariant reference constant"
                ),
            );
            error.combine(syn::Error::new(
                previous.invariant.id.span(),
                format!(
                    "the first generated invariant reference constant `{name}` is derived here"
                ),
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
                reference.invariant.id.span(),
                format!(
                    "generated invariant reference constant `{name}` conflicts with trait item `{item_ident}`"
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

const fn trait_item_identifier(item: &TraitItem) -> Option<&Ident> {
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

fn append(
    domain_path: &Path,
    item: &mut ItemTrait,
    references: Vec<GeneratedReference<'_>>,
) -> syn::Result<()> {
    for reference in references {
        let ident = reference.ident;
        let id = &reference.invariant.id;
        item.items.push(syn::parse2(quote_spanned! {id.span()=>
            #[doc(hidden)]
            #[allow(dead_code)]
            const #ident: #domain_path::InvariantReference =
                #domain_path::InvariantReference::__from_local(#id);
        })?);
    }
    Ok(())
}
