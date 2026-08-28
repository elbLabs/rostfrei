use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Ident, ItemImpl, Path, TypePath};

use super::attributes::Query;

pub fn assemble(
    domain_path: &Path,
    item: &ItemImpl,
    owner: &TypePath,
    group: &Ident,
    queries: &[Query],
) -> TokenStream {
    let descriptors = queries
        .iter()
        .map(|query| descriptor(domain_path, owner, query));
    let assertions = queries
        .iter()
        .map(|query| assertions(domain_path, owner, query));
    quote! {
        #item

        pub(crate) struct #group;

        impl #domain_path::QueryGroupType for #group {
            type Owner = #owner;

            const QUERIES: &'static [#domain_path::QueryDescriptor] = &[#(#descriptors),*];
        }

        #(#assertions)*
    }
}

fn descriptor(domain_path: &Path, owner: &TypePath, query: &Query) -> TokenStream {
    let id = &query.id;
    let label = &query.label;
    let signature = query.signature.as_ref().unwrap();
    let input = signature.input.as_ref().map_or_else(
        || quote!(None),
        |input| quote!(Some(<#input as #domain_path::QueryInputType<#owner>>::DESCRIPTOR)),
    );
    let output = &signature.output;
    quote! {
        #domain_path::QueryDescriptor {
            id: #domain_path::QueryId { aggregate: <#owner as #domain_path::AggregateType>::DESCRIPTOR.id, local: #id },
            label: #label,
            input: #input,
            output: <#output as #domain_path::QueryOutputType<#owner>>::DESCRIPTOR,
        }
    }
}

fn assertions(domain_path: &Path, owner: &TypePath, query: &Query) -> TokenStream {
    let root = &query.signature.as_ref().unwrap().root;
    let span = query.syntax.ident.span();
    quote_spanned! {span=>
        const _: () = {
            fn assert_owner<T: #domain_path::AggregateType>() {}
            fn assert_root(value: &<#owner as #domain_path::AggregateType>::Root) -> &<#owner as #domain_path::AggregateType>::Root { value }
            let _ = assert_owner::<#owner>;
            let _: fn(&#root) -> &#root = assert_root;
        };
    }
}
