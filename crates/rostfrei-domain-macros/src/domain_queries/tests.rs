#[test]
fn arbitrary_input_and_output_types_do_not_generate_dto_bounds() {
    let mut input = super::input::parse(quote::quote! {
        impl Aggregate {
            #[query(id = "lookup", label = "Lookup")]
            pub fn lookup(root: &Root, input: &ExternalInput) -> Vec<ExternalOutput> {
                Vec::new()
            }
        }
    })
    .expect("query impl");
    let mut queries = super::attributes::extract(&mut input.item.items).expect("query tags");
    super::validation::validate(&mut queries).expect("ordinary DTO signature");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.item,
        &input.owner,
        &syn::parse_quote!(Queries),
        &queries,
    )
    .to_string();

    assert!(output.contains("ExternalInput"));
    assert!(output.contains("ExternalOutput"));
    assert!(!output.contains("QueryInputType"));
    assert!(!output.contains("QueryOutputType"));
}
