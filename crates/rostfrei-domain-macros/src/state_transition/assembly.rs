use proc_macro2::TokenStream;
use quote::quote;
use syn::Path;

use super::ir::{Transition, TransitionSet};

pub fn assemble(domain_path: &Path, transition_set: &TransitionSet) -> TokenStream {
    let name = &transition_set.name;
    let state = &transition_set.state;
    let descriptors = transition_set
        .transitions
        .iter()
        .map(|transition| assemble_descriptor(domain_path, state, transition));
    let descriptor_arms = transition_set.transitions.iter().map(|transition| {
        let name = &transition.name;
        let descriptor = assemble_descriptor(domain_path, state, transition);
        quote! { Self::#name => &#descriptor }
    });

    quote! {
        impl #domain_path::StateTransition for #name {
            type State = #state;

            const DESCRIPTORS: &'static [#domain_path::StateTransitionDescriptor<Self::State>] =
                &[#(#descriptors),*];

            fn descriptor(
                &self,
            ) -> &'static #domain_path::StateTransitionDescriptor<Self::State> {
                match self {
                    #(#descriptor_arms),*
                }
            }
        }
    }
}

fn assemble_descriptor(
    domain_path: &Path,
    state: &syn::TypePath,
    transition: &Transition,
) -> TokenStream {
    let id = &transition.id;
    let label = &transition.label;
    let edges = transition.edges.iter().map(|edge| {
        let from = &edge.from;
        let to = &edge.to;
        quote! {
            #domain_path::StateTransitionEdge {
                from: #state::#from,
                to: #state::#to,
            }
        }
    });
    quote! {
        #domain_path::StateTransitionDescriptor {
            id: #domain_path::EntityLifecycleTransitionId {
                lifecycle: <#state as #domain_path::EntityLifecycleType>::DESCRIPTOR.id,
                local: #id,
            },
            label: #label,
            edges: &[#(#edges),*],
        }
    }
}
