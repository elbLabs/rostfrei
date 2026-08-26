use std::collections::HashMap;

use syn::{PathArguments, TypePath};

use super::ir::{Lifecycle, State};

pub fn validate(lifecycle: &Lifecycle) -> syn::Result<()> {
    crate::helper::id::validate(&lifecycle.id)?;
    crate::helper::label::validate(&lifecycle.label)?;
    validate_type_path(&lifecycle.owner, "lifecycle owner")?;
    validate_states(lifecycle)?;
    validate_initial(lifecycle)?;
    validate_transitions(lifecycle)
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

fn validate_initial(lifecycle: &Lifecycle) -> syn::Result<()> {
    if lifecycle
        .states
        .iter()
        .any(|state| state.name == lifecycle.initial)
    {
        Ok(())
    } else {
        Err(syn::Error::new(
            lifecycle.initial.span(),
            format!("unknown initial lifecycle state `{}`", lifecycle.initial),
        ))
    }
}

fn validate_transitions(lifecycle: &Lifecycle) -> syn::Result<()> {
    for state in &lifecycle.states {
        validate_state_transitions(state, &lifecycle.states)?;
    }
    Ok(())
}

fn validate_state_transitions(state: &State, states: &[State]) -> syn::Result<()> {
    let mut actions = HashMap::new();
    for transition in &state.transitions {
        if !states
            .iter()
            .any(|candidate| candidate.name == transition.target)
        {
            return Err(syn::Error::new(
                transition.target.span(),
                format!(
                    "unknown lifecycle transition target `{}`",
                    transition.target
                ),
            ));
        }
        if let Some(previous) = actions.insert(transition.action.lexical.clone(), transition) {
            let mut error = syn::Error::new(
                transition.action.span,
                format!(
                    "duplicate transition for state `{}` and action `{}`",
                    state.id.value(),
                    transition.action.lexical
                ),
            );
            error.combine(syn::Error::new(
                previous.action.span,
                "the first transition for this state and action is declared here",
            ));
            return Err(error);
        }
    }
    Ok(())
}

pub fn validate_type_path(path: &TypePath, subject: &str) -> syn::Result<()> {
    if path.qself.is_some()
        || path
            .path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, PathArguments::None))
    {
        return Err(syn::Error::new_spanned(
            path,
            format!("{subject} must be a direct, non-generic type path"),
        ));
    }
    Ok(())
}
