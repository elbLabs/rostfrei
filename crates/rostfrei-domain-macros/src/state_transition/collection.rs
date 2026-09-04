use syn::{DataEnum, Variant};

use super::ir::{Edge, Transition};

pub fn collect(data: &DataEnum) -> syn::Result<Vec<Transition>> {
    data.variants.iter().map(collect_transition).collect()
}

fn collect_transition(variant: &Variant) -> syn::Result<Transition> {
    let transition = super::transition_attribute::parse(variant)?;
    let edges = super::edge_attribute::parse(variant)?
        .into_iter()
        .map(|edge| Edge {
            from: edge.from,
            to: edge.to,
        })
        .collect();
    Ok(Transition {
        name: variant.ident.clone(),
        id: transition.id,
        label: transition.label,
        edges,
    })
}
