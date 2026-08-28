use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::{Item, parse_quote};

use super::subject::DomainTestSubjectInput;
use super::{DomainTestKind, validation};

pub fn expand(
    kind: DomainTestKind,
    args: TokenStream,
    input: TokenStream,
) -> syn::Result<TokenStream> {
    let subject = DomainTestSubjectInput::parse(kind, args)?;
    let mut function = match syn::parse2(input)? {
        Item::Fn(function) => function,
        item => {
            return Err(syn::Error::new_spanned(
                item,
                format!("{} tests may only be applied to a function", kind.name()),
            ));
        }
    };
    validation::validate_function(&function, kind)?;
    let companion_attributes = validation::companion_attributes(&function)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let function_name = function.sig.ident.clone();
    let companion_name = format_ident!(
        "__domain_test_metadata_{}_{}",
        kind.name(),
        function_name.unraw(),
        span = function_name.span()
    );
    let subject_name = format_ident!(
        "__DOMAIN_TEST_SUBJECT_{}_{}",
        kind.name().to_ascii_uppercase(),
        function_name.unraw(),
        span = function_name.span()
    );
    let subject = subject.assemble(&domain_path, kind);
    let file = quote_spanned!(function_name.span()=> file!());
    let line = quote_spanned!(function_name.span()=> line!());
    let column = quote_spanned!(function_name.span()=> column!());
    function.attrs.insert(0, parse_quote!(#[test]));

    Ok(quote! {
        #function

        #(#companion_attributes)*
        #[allow(non_upper_case_globals)]
        const #subject_name: #domain_path::DomainTestSubject = #subject;

        #(#companion_attributes)*
        #[test]
        #[ignore = "domain test metadata companion"]
        fn #companion_name() -> ::std::io::Result<()> {
            let descriptor = #domain_path::DomainTestDescriptor {
                package: env!("CARGO_PKG_NAME"),
                target: env!("CARGO_CRATE_NAME"),
                test: concat!(module_path!(), "::", stringify!(#function_name)),
                file: #file,
                line: #line,
                column: #column,
                subject: #subject_name,
            };
            #domain_path::__private::emit_domain_test_descriptor(descriptor)
        }
    })
}
