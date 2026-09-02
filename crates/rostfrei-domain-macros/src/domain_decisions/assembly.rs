use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{ItemImpl, Path, TypePath};

use super::{arguments::OwnerKind, decision::Decision};

pub fn assemble(
    domain_path: &Path,
    item: &ItemImpl,
    owner: &TypePath,
    group: &TypePath,
    decisions: &[Decision],
    owner_kind: OwnerKind,
) -> TokenStream {
    let impl_cfg_attributes = super::cfg_attributes::collect(&item.attrs);
    let descriptors = decisions
        .iter()
        .map(|decision| descriptor(domain_path, owner, decision));
    let references = decisions
        .iter()
        .map(|decision| reference(domain_path, group, decision));
    let signature_assertions = decisions
        .iter()
        .map(|decision| signature_assertion(domain_path, owner, decision, &impl_cfg_attributes));
    let owner_bound = match owner_kind {
        OwnerKind::Aggregate => quote!(#domain_path::AggregateDecisionOwnerType),
        OwnerKind::Entity => quote!(#domain_path::EntityDecisionOwnerType),
    };

    quote! {
        #item

        #(#impl_cfg_attributes)*
        impl #owner {
            #(#references)*
        }

        #(#signature_assertions)*

        #(#impl_cfg_attributes)*
        impl #domain_path::DecisionGroupType for #group {
            type Owner = #owner;

            const DECISIONS: &'static [#domain_path::DecisionDescriptor] = &[
                #(#descriptors),*
            ];
        }

        #(#impl_cfg_attributes)*
        const _: () = {
            fn assert_owner<T: #owner_bound>() {}
            let _ = assert_owner::<#owner>;
        };
    }
}

fn descriptor(domain_path: &Path, owner: &TypePath, decision: &Decision) -> TokenStream {
    let cfg_attributes = &decision.cfg_attributes;
    let id = &decision.id;
    let label = &decision.label;
    let return_type = &decision.return_type;
    quote! {
        #(#cfg_attributes)*
        #domain_path::DecisionDescriptor {
            id: #domain_path::DecisionId {
                owner: <#owner as #domain_path::DecisionOwnerType>::DECISION_OWNER_ID,
                local: #id,
            },
            label: #label,
            outcomes: <#return_type as #domain_path::DecisionOutcomeType>::OUTCOMES,
            implementation: #domain_path::DecisionImplementationDescriptor::Rust,
        }
    }
}

fn reference(domain_path: &Path, group: &TypePath, decision: &Decision) -> TokenStream {
    let id = &decision.id;
    let name = super::decision_reference_name::hidden_from_decision_id(id);
    let visibility = &decision.visibility;
    let cfg_attributes = &decision.cfg_attributes;
    quote_spanned! {id.span()=>
        #(#cfg_attributes)*
        #[doc(hidden)]
        #[allow(dead_code)]
        #visibility const #name: #domain_path::DecisionReference<#group> =
            #domain_path::DecisionReference::<#group>::__from_local(#id);
    }
}

fn signature_assertion(
    domain_path: &Path,
    owner: &TypePath,
    decision: &Decision,
    impl_cfg_attributes: &[syn::Attribute],
) -> TokenStream {
    let name = &decision.name;
    let cfg_attributes = &decision.cfg_attributes;
    let parameters = &decision.parameters;
    let return_type = &decision.return_type;
    quote_spanned! {name.span()=>
        #(#impl_cfg_attributes)*
        #(#cfg_attributes)*
        const _: () = {
            fn assert_outcome<T: #domain_path::DecisionOutcomeType>() {}
            let _ = assert_outcome::<#return_type>;
            let _: fn(#(#parameters),*) -> #return_type = #owner::#name;
        };
    }
}
