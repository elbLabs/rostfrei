use proc_macro2::TokenStream;
use quote::quote;
use syn::Path;

use super::ir::{Lifecycle, State};

pub fn assemble(domain_path: &Path, lifecycle: &Lifecycle) -> TokenStream {
    let name = &lifecycle.name;
    let id = &lifecycle.id;
    let label = &lifecycle.label;
    let states = lifecycle
        .states
        .iter()
        .map(|state| assemble_state(domain_path, id, state));

    quote! {
        impl #domain_path::EntityLifecycleType for #name {
            const DESCRIPTOR: #domain_path::EntityLifecycleDescriptor =
                #domain_path::EntityLifecycleDescriptor {
                    id: #domain_path::EntityLifecycleId(#id),
                    label: #label,
                    states: &[#(#states),*],
                };
        }
    }
}

fn assemble_state(domain_path: &Path, lifecycle_id: &syn::LitStr, state: &State) -> TokenStream {
    let id = &state.id;
    let label = &state.label;
    quote! {
        #domain_path::EntityLifecycleStateDescriptor {
            id: #domain_path::EntityLifecycleStateId {
                lifecycle: #domain_path::EntityLifecycleId(#lifecycle_id),
                local: #id,
            },
            label: #label,
        }
    }
}
