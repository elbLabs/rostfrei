use syn::DeriveInput;

#[test]
fn parses_only_semantic_service_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "pricing", label = "Pricing")]
        struct Pricing;
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");

    assert_eq!(attributes.id.value(), "pricing");
    assert_eq!(attributes.label.value(), "Pricing");
}

#[test]
fn rejects_removed_context_and_action_attachments() {
    for removed in [
        quote::quote!(context = Sales),
        quote::quote!(actions = [PricingActions]),
    ] {
        let input = syn::parse2::<DeriveInput>(quote::quote! {
            #[domain(id = "pricing", label = "Pricing", #removed)]
            struct Pricing;
        })
        .expect("derive input");
        assert!(super::attributes::Attributes::parse(&input.attrs).is_err());
    }
}

#[test]
fn descriptor_and_action_owners_forward_through_definition() {
    let attributes = super::attributes::Attributes {
        id: syn::LitStr::new("pricing", proc_macro2::Span::call_site()),
        label: syn::LitStr::new("Pricing", proc_macro2::Span::call_site()),
    };
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &syn::parse_quote!(Pricing),
        &attributes,
    )
    .to_string();

    assert!(output.contains("DomainServiceDefinition"));
    assert!(output.contains("DomainServiceActionOwnerType"));
    assert!(output.contains("ActionOwnerType"));
    assert!(!output.contains("ACTION_CONTRACTS"));
}
