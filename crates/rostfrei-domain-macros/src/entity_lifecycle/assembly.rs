use proc_macro2::TokenStream;
use quote::quote;

use super::ir::{Lifecycle, State, Transition};

pub fn assemble(lifecycle: &Lifecycle) -> TokenStream {
    let name = &lifecycle.name;
    let owner = &lifecycle.owner;
    let descriptor = assemble_descriptor(lifecycle);
    quote! {
        impl ::domain::EntityLifecycleType for #name {
            type Owner = #owner;

            const DESCRIPTOR: ::domain::EntityLifecycleDescriptor = #descriptor;
        }
    }
}

fn assemble_descriptor(lifecycle: &Lifecycle) -> TokenStream {
    let id = lifecycle_id(lifecycle);
    let label = &lifecycle.label;
    let states = lifecycle
        .states
        .iter()
        .map(|state| assemble_state(lifecycle, state));
    let initial = lifecycle
        .states
        .iter()
        .find(|state| state.name == lifecycle.initial)
        .unwrap();
    let initial = state_id(lifecycle, initial);
    let transitions = lifecycle.states.iter().flat_map(|state| {
        state
            .transitions
            .iter()
            .map(move |transition| assemble_transition(lifecycle, state, transition))
    });
    quote! {
        ::domain::EntityLifecycleDescriptor {
            id: #id,
            label: #label,
            states: &[#(#states),*],
            initial: #initial,
            transitions: &[#(#transitions),*],
        }
    }
}

fn assemble_state(lifecycle: &Lifecycle, state: &State) -> TokenStream {
    let id = state_id(lifecycle, state);
    let label = &state.label;
    quote! {
        ::domain::EntityLifecycleStateDescriptor {
            id: #id,
            label: #label,
        }
    }
}

fn assemble_transition(
    lifecycle: &Lifecycle,
    source: &State,
    transition: &Transition,
) -> TokenStream {
    let source = state_id(lifecycle, source);
    let target = lifecycle
        .states
        .iter()
        .find(|state| state.name == transition.target)
        .unwrap();
    let target = state_id(lifecycle, target);
    let action = super::action_reference::assemble_id(&transition.action, &lifecycle.owner);
    quote! {
        ::domain::EntityLifecycleTransitionDescriptor {
            source: #source,
            action: #action,
            target: #target,
        }
    }
}

fn lifecycle_id(lifecycle: &Lifecycle) -> TokenStream {
    let owner = &lifecycle.owner;
    let id = &lifecycle.id;
    quote! {
        ::domain::EntityLifecycleId {
            owner: <#owner as ::domain::EntityType>::DESCRIPTOR.id,
            local: #id,
        }
    }
}

fn state_id(lifecycle: &Lifecycle, state: &State) -> TokenStream {
    let lifecycle_id = lifecycle_id(lifecycle);
    let id = &state.id;
    quote! {
        ::domain::EntityLifecycleStateId {
            lifecycle: #lifecycle_id,
            local: #id,
        }
    }
}
