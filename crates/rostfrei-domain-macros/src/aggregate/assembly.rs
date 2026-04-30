use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::{aggregate_type, attributes::Attributes};

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let aggregate_type = aggregate_type::assemble(domain_path, name, attributes);
    let decision_owner = assemble_decision_owner(domain_path, name);
    let aggregate_decision_owner = assemble_aggregate_decision_owner(domain_path, name);
    quote! {
        #aggregate_type
        #decision_owner
        #aggregate_decision_owner
    }
}

fn assemble_aggregate_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::AggregateDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: #domain_path::DecisionOwnerId =
                #domain_path::DecisionOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}
