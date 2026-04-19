use proc_macro2::TokenStream;
use quote::quote;
use syn::Path;

use super::ir::{Lifecycle, State, Transition};

struct ResolvedLifecycle<'a> {
    lifecycle: &'a Lifecycle,
    initial: &'a State,
    transitions: Vec<ResolvedTransition<'a>>,
}

struct ResolvedTransition<'a> {
    source: &'a State,
    transition: &'a Transition,
    target: &'a State,
}

pub fn assemble(domain_path: &Path, lifecycle: &Lifecycle) -> TokenStream {
    let lifecycle = match resolve(lifecycle) {
        Ok(lifecycle) => lifecycle,
        Err(error) => return error.into_compile_error(),
    };
    let name = &lifecycle.lifecycle.name;
    let owner = &lifecycle.lifecycle.owner;
    let descriptor = assemble_descriptor(domain_path, &lifecycle);
    quote! {
        impl #domain_path::EntityLifecycleType for #name {
            type Owner = #owner;

            const DESCRIPTOR: #domain_path::EntityLifecycleDescriptor = #descriptor;
        }
    }
}

fn resolve(lifecycle: &Lifecycle) -> syn::Result<ResolvedLifecycle<'_>> {
    let initial = lifecycle
        .states
        .iter()
        .find(|state| state.name == lifecycle.initial)
        .ok_or_else(|| {
            syn::Error::new(
                lifecycle.initial.span(),
                format!("unknown initial lifecycle state `{}`", lifecycle.initial),
            )
        })?;
    let mut transitions = Vec::new();
    for source in &lifecycle.states {
        for transition in &source.transitions {
            let target = lifecycle
                .states
                .iter()
                .find(|state| state.name == transition.target)
                .ok_or_else(|| {
                    syn::Error::new(
                        transition.target.span(),
                        format!(
                            "unknown lifecycle transition target `{}`",
                            transition.target
                        ),
                    )
                })?;
            transitions.push(ResolvedTransition {
                source,
                transition,
                target,
            });
        }
    }
    Ok(ResolvedLifecycle {
        lifecycle,
        initial,
        transitions,
    })
}

fn assemble_descriptor(domain_path: &Path, resolved: &ResolvedLifecycle<'_>) -> TokenStream {
    let lifecycle = resolved.lifecycle;
    let id = lifecycle_id(domain_path, lifecycle);
    let label = &lifecycle.label;
    let states = lifecycle
        .states
        .iter()
        .map(|state| assemble_state(domain_path, lifecycle, state));
    let initial = state_id(domain_path, lifecycle, resolved.initial);
    let transitions = resolved
        .transitions
        .iter()
        .map(|transition| assemble_transition(domain_path, lifecycle, transition));
    quote! {
        #domain_path::EntityLifecycleDescriptor {
            id: #id,
            label: #label,
            states: &[#(#states),*],
            initial: #initial,
            transitions: &[#(#transitions),*],
        }
    }
}

fn assemble_state(domain_path: &Path, lifecycle: &Lifecycle, state: &State) -> TokenStream {
    let id = state_id(domain_path, lifecycle, state);
    let label = &state.label;
    quote! {
        #domain_path::EntityLifecycleStateDescriptor {
            id: #id,
            label: #label,
        }
    }
}

fn assemble_transition(
    domain_path: &Path,
    lifecycle: &Lifecycle,
    transition: &ResolvedTransition<'_>,
) -> TokenStream {
    let source = state_id(domain_path, lifecycle, transition.source);
    let target = state_id(domain_path, lifecycle, transition.target);
    let action = super::action_reference::assemble_id(
        domain_path,
        &transition.transition.action,
        &lifecycle.owner,
    );
    quote! {
        #domain_path::EntityLifecycleTransitionDescriptor {
            source: #source,
            action: #action,
            target: #target,
        }
    }
}

fn lifecycle_id(domain_path: &Path, lifecycle: &Lifecycle) -> TokenStream {
    let owner = &lifecycle.owner;
    let id = &lifecycle.id;
    quote! {
        #domain_path::EntityLifecycleId {
            owner: <#owner as #domain_path::EntityType>::DESCRIPTOR.id,
            local: #id,
        }
    }
}

fn state_id(domain_path: &Path, lifecycle: &Lifecycle, state: &State) -> TokenStream {
    let lifecycle_id = lifecycle_id(domain_path, lifecycle);
    let id = &state.id;
    quote! {
        #domain_path::EntityLifecycleStateId {
            lifecycle: #lifecycle_id,
            local: #id,
        }
    }
}
