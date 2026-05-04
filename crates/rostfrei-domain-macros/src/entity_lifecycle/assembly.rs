use proc_macro2::TokenStream;
use quote::quote;
use syn::Path;

use super::ir::{Lifecycle, State};

pub fn assemble(domain_path: &Path, lifecycle: &Lifecycle) -> syn::Result<TokenStream> {
    let name = &lifecycle.name;
    let id = &lifecycle.id;
    let label = &lifecycle.label;
    let initial_name = &lifecycle.initial;
    let initial = lifecycle
        .states
        .iter()
        .find(|state| state.name == lifecycle.initial)
        .ok_or_else(|| {
            syn::Error::new_spanned(
                &lifecycle.initial,
                "initial lifecycle state must name a declared variant",
            )
        })?;
    let initial_id = assemble_state_id(domain_path, id, &initial.id);
    let states = lifecycle
        .states
        .iter()
        .map(|state| assemble_state(domain_path, id, state));
    let state_ids = lifecycle.states.iter().map(|state| {
        let state_name = &state.name;
        let state_id = assemble_state_id(domain_path, id, &state.id);
        quote! { Self::#state_name => #state_id }
    });

    Ok(quote! {
        impl #domain_path::EntityLifecycleType for #name {
            const DESCRIPTOR: #domain_path::EntityLifecycleDescriptor =
                #domain_path::EntityLifecycleDescriptor {
                    id: #domain_path::EntityLifecycleId(#id),
                    label: #label,
                    initial: #initial_id,
                    states: &[#(#states),*],
                };
        }

        impl #domain_path::LifecycleState for #name {
            const INITIAL: Self = Self::#initial_name;

            fn state_id(self) -> #domain_path::EntityLifecycleStateId {
                match self {
                    #(#state_ids),*
                }
            }
        }
    })
}

fn assemble_state(domain_path: &Path, lifecycle_id: &syn::LitStr, state: &State) -> TokenStream {
    let id = assemble_state_id(domain_path, lifecycle_id, &state.id);
    let label = &state.label;
    quote! {
        #domain_path::EntityLifecycleStateDescriptor {
            id: #id,
            label: #label,
        }
    }
}

fn assemble_state_id(
    domain_path: &Path,
    lifecycle_id: &syn::LitStr,
    state_id: &syn::LitStr,
) -> TokenStream {
    quote! {
        #domain_path::EntityLifecycleStateId {
            lifecycle: #domain_path::EntityLifecycleId(#lifecycle_id),
            local: #state_id,
        }
    }
}
