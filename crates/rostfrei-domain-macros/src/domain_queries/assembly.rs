use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Ident, ItemImpl, TypePath};

use super::attributes::Query;

pub fn assemble(item: ItemImpl, owner: &TypePath, group: &Ident, queries: &[Query]) -> TokenStream {
    let descriptors = queries.iter().map(|query| descriptor(owner, query));
    let assertions = queries.iter().map(|query| assertions(owner, query));
    quote! {
        #item

        pub(crate) struct #group;

        impl ::rostfrei_domain::QueryGroupType for #group {
            type Owner = #owner;

            const QUERIES: &'static [::rostfrei_domain::QueryDescriptor] = &[#(#descriptors),*];
        }

        #(#assertions)*
    }
}

fn descriptor(owner: &TypePath, query: &Query) -> TokenStream {
    let id = &query.id;
    let label = &query.label;
    let signature = query.signature.as_ref().unwrap();
    let input = signature.input.as_ref().map_or_else(
        || quote!(None),
        |input| quote!(Some(<#input as ::rostfrei_domain::QueryInputType<#owner>>::DESCRIPTOR)),
    );
    let output = &signature.output;
    quote! {
        ::rostfrei_domain::QueryDescriptor {
            id: ::rostfrei_domain::QueryId { aggregate: <#owner as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id, local: #id },
            label: #label,
            input: #input,
            output: <#output as ::rostfrei_domain::QueryOutputType<#owner>>::DESCRIPTOR,
        }
    }
}

fn assertions(owner: &TypePath, query: &Query) -> TokenStream {
    let root = &query.signature.as_ref().unwrap().root;
    let span = query.syntax.ident.span();
    quote_spanned! {span=>
        const _: () = {
            fn assert_owner<T: ::rostfrei_domain::AggregateType>() {}
            let _ = assert_owner::<#owner>;
            fn assert_root(value: &<#owner as ::rostfrei_domain::AggregateType>::Root) -> &<#owner as ::rostfrei_domain::AggregateType>::Root { value }
            let _: fn(&#root) -> &#root = assert_root;
        };
    }
}
