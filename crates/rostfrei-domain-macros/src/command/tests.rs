use syn::DeriveInput;

#[test]
fn defaults_schema_version_to_one() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "create", label = "Create")]
        struct Create;
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert_eq!(attributes.schema_version.base10_parse::<u32>().unwrap(), 1);
}

#[test]
fn accepts_an_explicit_non_default_schema_version() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "create", label = "Create", schema_version = 2)]
        struct Create;
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert_eq!(attributes.schema_version.base10_parse::<u32>().unwrap(), 2);
}

#[test]
fn rejects_removed_command_relationship_and_codegen_flags() {
    for removed in [
        quote::quote!(owner = Aggregate),
        quote::quote!(rejection = Rejected),
        quote::quote!(json),
        quote::quote!(runtime),
    ] {
        let input = syn::parse2::<DeriveInput>(quote::quote! {
            #[domain(id = "create", label = "Create", #removed)]
            struct Create;
        })
        .expect("derive input");
        assert!(super::attributes::Attributes::parse(&input.attrs).is_err());
    }
}

#[test]
fn always_generates_semantic_metadata_and_exact_json_codec() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "create", label = "Create")]
        struct Create { name: String, optional: Option<u64> }
    };
    let syntax_fields = super::input::extract(&input).expect("command shape");
    let fields = crate::field::extract(syntax_fields).expect("fields");
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.ident,
        &attributes,
        &fields,
        syntax_fields,
    )
    .to_string();

    assert!(output.contains("impl :: domain :: Command for Create"));
    assert!(output.contains("impl :: domain :: JsonCommandPayload for Create"));
    assert!(!output.contains("CommandDefinition"));
    assert!(!output.contains("type Owner"));
}
