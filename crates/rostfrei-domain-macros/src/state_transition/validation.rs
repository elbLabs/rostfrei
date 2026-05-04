use std::collections::HashMap;

use super::ir::TransitionSet;

pub fn validate(transition_set: &TransitionSet) -> syn::Result<()> {
    let mut ids = HashMap::new();
    for transition in &transition_set.transitions {
        crate::helper::id::validate(&transition.id)?;
        crate::helper::label::validate(&transition.label)?;
        let id = transition.id.value();
        if let Some(previous) = ids.insert(id, transition) {
            let mut error = syn::Error::new(transition.id.span(), "duplicate state transition id");
            error.combine(syn::Error::new(
                previous.id.span(),
                "the first state transition id is declared here",
            ));
            return Err(error);
        }

        validate_unique_sources(transition)?;
    }
    Ok(())
}

fn validate_unique_sources(transition: &super::ir::Transition) -> syn::Result<()> {
    let mut sources = HashMap::new();
    for edge in &transition.edges {
        let source = edge.from.to_string();
        if let Some(previous) = sources.insert(source, edge) {
            let mut error =
                syn::Error::new_spanned(&edge.from, "duplicate source state for state transition");
            error.combine(syn::Error::new_spanned(
                &previous.from,
                "the first edge from this state is declared here",
            ));
            return Err(error);
        }
    }
    Ok(())
}
