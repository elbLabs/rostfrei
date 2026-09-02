#[test]
fn accepts_arbitrary_parameter_types_and_patterns_without_metadata() {
    let signature: syn::Signature = syn::parse_quote! {
        fn decide(
            (left, right): (External, External),
            mut history: Vec<&'static External>,
        ) -> Outcome
    };
    let parsed = super::signature::parse(&signature).expect("ordinary parameters");

    assert_eq!(parsed.parameters.len(), 2);
}

#[test]
fn generated_descriptor_keeps_only_semantic_decision_metadata() {
    let mut input = super::input::parse(quote::quote! {
        impl Owner {
            #[decision(id = "choose", label = "Choose")]
            fn choose((left, right): (External, External)) -> Outcome {
                Outcome::Selected(left, right)
            }
        }
    })
    .expect("decision impl");
    let decisions = super::decision_collection::collect(&mut input.item.items).expect("decisions");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.item,
        &input.owner,
        &syn::parse_quote!(Decisions),
        &decisions,
        super::arguments::OwnerKind::Aggregate,
    )
    .to_string();

    assert!(output.contains("DecisionOutcomeType"));
    assert!(!output.contains("DecisionInputType"));
    assert!(!output.contains("DecisionParameterDescriptor"));
}
