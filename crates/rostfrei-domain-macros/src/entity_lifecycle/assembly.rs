use proc_macro2::TokenStream;
use quote::quote;
use syn::Path;

use super::ir::{Lifecycle, State, Transition};

pub fn assemble(domain_path: &Path, lifecycle: &Lifecycle) -> TokenStream {
    let name = &lifecycle.name;
    let owner = &lifecycle.owner;
    let descriptor = assemble_descriptor(domain_path, lifecycle);
    quote! {
        impl #domain_path::EntityLifecycleType for #name {
            type Owner = #owner;

            const DESCRIPTOR: #domain_path::EntityLifecycleDescriptor = #descriptor;
        }
    }
}

fn assemble_descriptor(domain_path: &Path, lifecycle: &Lifecycle) -> TokenStream {
    let id = lifecycle_id(domain_path, lifecycle);
    let label = &lifecycle.label;
    let states = lifecycle
        .states
        .iter()
        .map(|state| assemble_state(domain_path, lifecycle, state));
    let initial = lifecycle
        .states
        .iter()
        .find(|state| state.name == lifecycle.initial)
        .unwrap();
    let initial = state_id(domain_path, lifecycle, initial);
    let transitions = lifecycle.states.iter().flat_map(|state| {
        state
            .transitions
            .iter()
            .map(move |transition| assemble_transition(domain_path, lifecycle, state, transition))
    });
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
    source: &State,
    transition: &Transition,
) -> TokenStream {
    let source = state_id(domain_path, lifecycle, source);
    let target = lifecycle
        .states
        .iter()
        .find(|state| state.name == transition.target)
        .unwrap();
    let target = state_id(domain_path, lifecycle, target);
    let action =
        super::action_reference::assemble_id(domain_path, &transition.action, &lifecycle.owner);
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
