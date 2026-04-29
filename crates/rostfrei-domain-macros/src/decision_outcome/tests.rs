use syn::DeriveInput;

#[test]
fn accepts_arbitrary_payload_fields_without_shape_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        enum Outcome {
            #[outcome(id = "selected", label = "Selected")]
            Selected(HashMap<String, External>, &'static External),
            #[outcome(id = "rejected", label = "Rejected")]
            Rejected { reason: ExternalReason },
        }
    };
    let data = super::input::validate(&input).expect("outcome enum");
    let outcomes = super::input::collect(data).expect("outcome tags");
    super::validation::validate(&outcomes).expect("valid outcome metadata");
    let output = super::assembly::assemble(&syn::parse_quote!(::domain), &input.ident, &outcomes)
        .to_string();

    assert_eq!(outcomes.len(), 2);
    assert!(!output.contains("shape"));
    assert!(!output.contains("DecisionOutcomeValueType"));
}
