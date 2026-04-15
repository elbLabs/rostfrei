use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{ItemImpl, LitStr, Path, TypePath};

use super::{arguments::OwnerKind, decision::Decision};

pub fn assemble(
    domain_path: &Path,
    item: &ItemImpl,
    owner: &TypePath,
    decisions: &[Decision],
    owner_kind: OwnerKind,
) -> TokenStream {
    let impl_cfg_attributes = super::cfg_attributes::collect(&item.attrs);
    let descriptors = decisions
        .iter()
        .map(|decision| descriptor(domain_path, owner, decision));
    let references = decisions
        .iter()
        .map(|decision| reference(domain_path, owner, decision));
    let signature_assertions = decisions
        .iter()
        .map(|decision| signature_assertion(owner, decision, &impl_cfg_attributes));
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
        impl #domain_path::__private::DecisionProvider for #owner {
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
    let parameters = decision.parameters.iter().map(|parameter| {
        let name = LitStr::new(
            parameter.name.to_string().trim_start_matches("r#"),
            parameter.name.span(),
        );
        let ty = &parameter.ty;
        quote! {
            #domain_path::DecisionParameterDescriptor {
                name: #name,
                input: <#ty as #domain_path::DecisionInputType>::DESCRIPTOR,
            }
        }
    });
    let output = &decision.output;
    let error = &decision.error;
    quote! {
        #(#cfg_attributes)*
        #domain_path::DecisionDescriptor {
            id: #domain_path::DecisionId {
                owner: <#owner as #domain_path::DecisionOwnerType>::DECISION_OWNER_ID,
                local: #id,
            },
            label: #label,
            parameters: &[#(#parameters),*],
            output: <#output as #domain_path::DecisionOutputType>::DESCRIPTOR,
            error: <#error as #domain_path::DecisionOutputType>::DESCRIPTOR,
            implementation: #domain_path::DecisionImplementationDescriptor::Rust,
        }
    }
}

fn reference(domain_path: &Path, owner: &TypePath, decision: &Decision) -> TokenStream {
    let id = &decision.id;
    let name = super::decision_reference_name::hidden_from_decision_id(id);
    let visibility = &decision.visibility;
    let cfg_attributes = &decision.cfg_attributes;
    quote_spanned! {id.span()=>
        #(#cfg_attributes)*
        #[doc(hidden)]
        #[allow(dead_code)]
        #visibility const #name: #domain_path::DecisionReference<#owner> =
            #domain_path::DecisionReference::<#owner>::__from_local(#id);
    }
}

fn signature_assertion(
    owner: &TypePath,
    decision: &Decision,
    impl_cfg_attributes: &[syn::Attribute],
) -> TokenStream {
    let name = &decision.name;
    let cfg_attributes = &decision.cfg_attributes;
    let parameters = decision.parameters.iter().map(|parameter| &parameter.ty);
    let output = &decision.output;
    let error = &decision.error;
    quote_spanned! {name.span()=>
        #(#impl_cfg_attributes)*
        #(#cfg_attributes)*
        const _: () = {
            let _: fn(#(#parameters),*) -> ::core::result::Result<#output, #error> =
                #owner::#name;
        };
    }
}
