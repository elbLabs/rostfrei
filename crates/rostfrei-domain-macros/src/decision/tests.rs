use quote::quote;

fn arguments() -> proc_macro2::TokenStream {
    quote!(id = "rental-eligibility", label = "Rental eligibility")
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
        pub trait RentalEligibilityDecision {
            fn assess<T>(&self, input: &T) -> Result<Eligibility<T>, Rejection>;
        }
    })
    .expect("valid decision")
    .to_string();
    assert!(output.contains("pub trait RentalEligibilityDecision"));
    assert!(output.contains("fn assess < T >"));
    assert!(output.contains("const LOCAL_ID"));
    assert!(output.contains("const LABEL"));
    assert!(output.contains("const DESCRIPTOR"));
    assert!(output.contains("DecisionId"));
    assert!(!output.contains("Owner"));
    assert!(!output.contains("Group"));
}

#[test]
fn preserves_generics_inheritance_and_signature_details() {
    let output = expand(quote! {
        pub unsafe trait Decision<'a, T>: Send + Parent<T> where T: 'a {
            type Outcome;
            fn decide<P>(&self, input: &'a T, project: P) -> Self::Outcome
            where P: FnOnce(&T);
        }
    })
    .expect("valid decision")
    .to_string();
    assert!(output.contains("pub unsafe trait Decision < 'a , T > : Send + Parent < T >"));
    assert!(output.contains("where T : 'a"));
    assert!(output.contains("fn decide < P >"));
}

#[test]
fn rejects_invalid_target_metadata_and_reserved_names() {
    assert!(
        super::expand::expand(
            &arguments(),
            quote!(
                struct Decision;
            )
        )
        .expect_err("non-trait")
        .to_string()
        .contains("may only be applied to a trait")
    );
    for arguments in [
        quote!(label = "Decision"),
        quote!(id = "Decision", label = "Decision"),
        quote!(id = "decision", label = "Decision", owner = Fleet),
        quote!(id = "decision", label = "Decision", group = Decisions),
    ] {
        assert!(
            super::expand::expand(
                &arguments,
                quote!(
                    trait Decision {}
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
        let error = super::expand::expand(&arguments(), quote! { trait Decision { #item } })
            .expect_err("reserved item");
        assert!(error.to_string().contains("reserved domain_decision"));
    }
}
