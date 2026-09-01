use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{Ident, ItemImpl, Path, TypePath};

use super::{attributes::Query, signature::ParsedSignature};

struct AssemblyQuery<'a> {
    query: &'a Query,
    signature: &'a ParsedSignature,
}

pub fn assemble(
    domain_path: &Path,
    item: &ItemImpl,
    owner: &TypePath,
    group: &Ident,
    queries: &[Query],
) -> TokenStream {
    let queries = match validated_queries(queries) {
        Ok(queries) => queries,
        Err(error) => return error.into_compile_error(),
    };
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

fn validated_queries(queries: &[Query]) -> syn::Result<Vec<AssemblyQuery<'_>>> {
    queries
        .iter()
        .map(|query| {
            let signature = query.signature.as_ref().ok_or_else(|| {
                syn::Error::new_spanned(
                    &query.syntax,
                    "query signature must be validated before assembly",
                )
            })?;
            Ok(AssemblyQuery { query, signature })
        })
        .collect()
}

fn descriptor(domain_path: &Path, owner: &TypePath, query: &AssemblyQuery<'_>) -> TokenStream {
    let id = &query.query.id;
    let label = &query.query.label;
    let input = query.signature.input.as_ref().map_or_else(
        || quote!(None),
        |input| quote!(Some(<#input as #domain_path::QueryInputType<#owner>>::DESCRIPTOR)),
    );
    let output = &query.signature.output;
    quote! {
        #domain_path::QueryDescriptor {
            id: #domain_path::QueryId { aggregate: <#owner as #domain_path::AggregateType>::DESCRIPTOR.id, local: #id },
            label: #label,
            input: #input,
            output: <#output as #domain_path::QueryOutputType<#owner>>::DESCRIPTOR,
        }
    }
}

fn assertions(domain_path: &Path, owner: &TypePath, query: &AssemblyQuery<'_>) -> TokenStream {
    let root = &query.signature.root;
    let span = query.query.syntax.ident.span();
    quote_spanned! {span=>
        const _: () = {
            fn assert_owner<T: #domain_path::AggregateDefinition>() {}
            fn assert_root(value: &<#owner as #domain_path::AggregateDefinition>::Root) -> &<#owner as #domain_path::AggregateDefinition>::Root { value }
            let _ = assert_owner::<#owner>;
            let _: fn(&#root) -> &#root = assert_root;
        };
    }
}
