use std::collections::HashMap;

use super::ir::Lifecycle;

pub fn validate(lifecycle: &Lifecycle) -> syn::Result<()> {
    crate::helper::id::validate(&lifecycle.id)?;
    crate::helper::label::validate(&lifecycle.label)?;
    validate_states(lifecycle)?;
    validate_initial_state(lifecycle)
}

fn validate_states(lifecycle: &Lifecycle) -> syn::Result<()> {
    let mut ids = HashMap::new();
    for state in &lifecycle.states {
        crate::helper::id::validate(&state.id)?;
        crate::helper::label::validate(&state.label)?;
        let id = state.id.value();
        if let Some(previous) = ids.insert(id.clone(), state) {
            let mut error = syn::Error::new(state.id.span(), "duplicate lifecycle state id");
            error.combine(syn::Error::new(
                previous.id.span(),
                "the first lifecycle state id is declared here",
            ));
            return Err(error);
        }
    }
    Ok(())
}

fn validate_initial_state(lifecycle: &Lifecycle) -> syn::Result<()> {
    if lifecycle
        .states
        .iter()
        .any(|state| state.name == lifecycle.initial)
    {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        &lifecycle.initial,
        "initial lifecycle state must name a declared variant",
    ))
}
