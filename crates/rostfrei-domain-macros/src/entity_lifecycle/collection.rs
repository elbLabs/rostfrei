use syn::{DataEnum, Variant};

use super::ir::State;

pub fn collect(data: &DataEnum) -> syn::Result<Vec<State>> {
    data.variants.iter().map(collect_state).collect()
}

fn collect_state(variant: &Variant) -> syn::Result<State> {
    let attribute = super::state_attribute::parse(variant)?;
    Ok(State {
        name: variant.ident.clone(),
        id: attribute.id,
        label: attribute.label,
    })
}
