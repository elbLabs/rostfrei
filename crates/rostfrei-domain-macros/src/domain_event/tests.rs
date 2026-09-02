use syn::DeriveInput;

#[test]
fn defaults_schema_version_to_one() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "created", label = "Created")]
        struct Created;
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert!(attributes.schema_version.is_none());

    let fields = crate::field::extract(super::input::extract(&input).unwrap()).unwrap();
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.ident,
        &attributes,
        &fields,
    )
    .to_string();
    assert!(!output.contains("SCHEMA_VERSION"));
}

#[test]
fn preserves_an_explicit_evolution_schema_version() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "created", label = "Created", schema_version = 2)]
        struct Created;
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert_eq!(
        attributes
            .schema_version
            .as_ref()
            .unwrap()
            .base10_parse::<u32>()
            .unwrap(),
        2
    );

    let fields = crate::field::extract(super::input::extract(&input).unwrap()).unwrap();
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.ident,
        &attributes,
        &fields,
    )
    .to_string();
    assert!(output.contains("SCHEMA_VERSION"));
}

#[test]
fn generates_the_public_semantic_event_trait_directly() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "created", label = "Created")]
        struct Created { value: ExternalPayload }
    };
    let syntax_fields = super::input::extract(&input).expect("event shape");
    let fields = crate::field::extract(syntax_fields).expect("fields");
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &input.ident,
        &attributes,
        &fields,
    )
    .to_string();

    assert!(output.contains("impl :: domain :: DomainEvent for Created"));
    assert!(!output.contains("DomainEventDefinition"));
    assert!(!output.contains("SCHEMA_VERSION"));
}
