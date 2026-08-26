use syn::{DataEnum, Variant};

use super::ir::{State, Transition};

pub fn collect(data: &DataEnum) -> syn::Result<Vec<State>> {
    data.variants.iter().map(collect_state).collect()
}

fn collect_state(variant: &Variant) -> syn::Result<State> {
    let attribute = super::state_attribute::parse(variant)?;
    Ok(State {
        name: variant.ident.clone(),
        id: attribute.id,
        label: attribute.label,
        transitions: collect_transitions(variant)?,
    })
}

fn collect_transitions(variant: &Variant) -> syn::Result<Vec<Transition>> {
    variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("transition"))
        .map(super::transition_attribute::parse)
        .collect()
}
