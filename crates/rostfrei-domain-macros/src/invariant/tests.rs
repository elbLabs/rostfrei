use quote::quote;

fn arguments() -> proc_macro2::TokenStream {
    quote!(id = "unique-bicycles", label = "Unique bicycles")
}

fn expand(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let attributes = super::attributes::Attributes::parse(&arguments())?;
    super::validation::validate(&attributes)?;
    let item: syn::ItemTrait = syn::parse2(input)?;
    super::validation::validate_trait(&item)?;
    Ok(super::assembly::assemble(
        &syn::parse_quote!(::domain),
        item,
        &attributes,
    ))
}

#[test]
fn injects_only_global_semantic_metadata() {
    let output = expand(quote! {
        pub trait UniqueBicyclesInvariant {
            fn holds<T>(&self, values: &[T]) -> bool where T: Eq;
        }
    })
    .expect("valid invariant")
    .to_string();
    assert!(output.contains("pub trait UniqueBicyclesInvariant"));
    assert!(output.contains("fn holds < T >"));
    assert!(output.contains("const LOCAL_ID"));
    assert!(output.contains("const LABEL"));
    assert!(output.contains("const DESCRIPTOR"));
    assert!(output.contains("InvariantId"));
    assert!(!output.contains("Owner"));
}

#[test]
fn preserves_generics_inheritance_and_signature_details() {
    let output = expand(quote! {
        pub unsafe trait Invariant<'a, T>: Send + Parent<T> where T: 'a {
            type Violation;
            fn check<P>(&self, input: &'a T, project: P) -> Result<(), Self::Violation>
            where P: FnOnce(&T);
        }
    })
    .expect("valid invariant")
    .to_string();
    assert!(output.contains("pub unsafe trait Invariant < 'a , T > : Send + Parent < T >"));
    assert!(output.contains("where T : 'a"));
    assert!(output.contains("fn check < P >"));
}

#[test]
fn rejects_invalid_target_metadata_and_reserved_names() {
    assert!(
        super::expand::expand(
            &arguments(),
            quote!(
                struct Invariant;
            )
        )
        .expect_err("non-trait")
        .to_string()
        .contains("may only be applied to a trait")
    );
    for arguments in [
        quote!(label = "Invariant"),
        quote!(id = "Invariant", label = "Invariant"),
        quote!(id = "invariant", label = "Invariant", owner = Fleet),
    ] {
        assert!(
            super::expand::expand(
                &arguments,
                quote!(
                    trait Invariant {}
                )
            )
            .is_err()
        );
    }
    for item in [
        quote!(
            const LOCAL_ID: &'static str = "custom";
        ),
        quote!(
            fn LABEL();
        ),
        quote!(
            type DESCRIPTOR;
        ),
    ] {
        let error = super::expand::expand(&arguments(), quote! { trait Invariant { #item } })
            .expect_err("reserved item");
        assert!(error.to_string().contains("reserved domain_invariant"));
    }
}
