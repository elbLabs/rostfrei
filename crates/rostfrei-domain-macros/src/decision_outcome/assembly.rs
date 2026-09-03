use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::ir::Outcome;

pub fn assemble(domain_path: &Path, name: &Ident, outcomes: &[Outcome]) -> TokenStream {
    let descriptors = outcomes.iter().map(|outcome| {
        let cfg_attributes = &outcome.cfg_attributes;
        let local_id = &outcome.local_id;
        let label = &outcome.label;
        quote! {
            #(#cfg_attributes)*
            #domain_path::DecisionOutcomeDescriptor {
                local_id: #local_id,
                label: #label,
            }
        }
    });
    quote! {
        impl #domain_path::DecisionOutcomeType for #name {
            const OUTCOMES: &'static [#domain_path::DecisionOutcomeDescriptor] = &[
                #(#descriptors),*
            ];
        }
    }
}
