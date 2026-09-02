use syn::DeriveInput;

fn input_with(metadata: &proc_macro2::TokenStream) -> DeriveInput {
    syn::parse2(quote::quote! {
        #[domain(
            id = "unavailable",
            label = "Unavailable",
            code = "UNAVAILABLE",
            message = "The item is unavailable.",
            #metadata
        )]
        struct Unavailable { item: String }
    })
    .expect("derive input")
}

#[test]
fn parses_owner_independent_error_metadata() {
    let input = input_with(&quote::quote!());
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert_eq!(attributes.id.value(), "unavailable");
    assert_eq!(attributes.code.value(), "UNAVAILABLE");
}

#[test]
fn rejects_removed_owner_and_json_attributes() {
    assert!(
        super::attributes::Attributes::parse(&input_with(&quote::quote!(owner = Aggregate)).attrs)
            .is_err()
    );
    assert!(super::attributes::Attributes::parse(&input_with(&quote::quote!(json)).attrs).is_err());
}

#[test]
fn always_generates_semantic_metadata_and_json_encoding() {
    let input = input_with(&quote::quote!());
    let syntax_fields = super::input::extract(&input).expect("error shape");
    let fields = crate::field::extract(syntax_fields).expect("fields");
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    super::validation::validate(&attributes, &fields).expect("valid error");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.ident,
        &attributes,
        &fields,
        syntax_fields,
    )
    .to_string();

    assert!(output.contains("impl :: domain :: DomainError for Unavailable"));
    assert!(output.contains("impl :: domain :: JsonErrorPayload for Unavailable"));
    assert!(!output.contains("type Owner"));
}
