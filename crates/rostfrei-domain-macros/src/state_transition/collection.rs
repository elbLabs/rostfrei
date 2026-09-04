use syn::{DataEnum, Variant};

use super::ir::Transition;

pub fn collect(data: &DataEnum) -> syn::Result<Vec<Transition>> {
    data.variants.iter().map(collect_transition).collect()
}

fn collect_transition(variant: &Variant) -> syn::Result<Transition> {
    let attribute = super::edge_attribute::parse(variant)?;
    Ok(Transition {
        name: variant.ident.clone(),
        id: attribute.id,
        label: attribute.label,
        from: attribute.from,
        to: attribute.to,
    })
}
