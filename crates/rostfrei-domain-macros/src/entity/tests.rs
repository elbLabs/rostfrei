use syn::DeriveInput;

#[test]
fn accepts_identity_tag_and_definition_owned_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "bicycle", label = "Bicycle")]
        struct Bicycle {
            #[domain(identity)]
            id: BicycleId,
            active: bool,
        }
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    let fields =
        crate::field::extract(super::input::extract(&input).expect("input")).expect("fields");
    let identity = super::validation::validate(&fields).expect("identity");

    assert_eq!(attributes.id.value(), "bicycle");
    assert_eq!(identity.name.value(), "id");
}

#[test]
fn rejects_removed_entity_relationship_attributes() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "bicycle", label = "Bicycle", owner = Fleet)]
        struct Bicycle {
            #[domain(identity)]
            id: BicycleId,
        }
    };

    assert!(super::attributes::Attributes::parse(&input.attrs).is_err());
}

#[test]
fn entity_generates_the_scoped_identity_binding() {
    let identity = crate::field::Field {
        name: syn::LitStr::new("id", proc_macro2::Span::call_site()),
        member: syn::Member::Named(syn::parse_quote!(id)),
        base: syn::parse_quote!(BicycleId),
        wrappers: Vec::new(),
        role: crate::field::Role::Identity,
    };
    let output = super::identity::assemble(
        &syn::parse_quote!(::domain),
        &syn::parse_quote!(Bicycle),
        &identity,
    )
    .to_string();

    assert!(output.contains("__private :: DomainIdentityType for BicycleId"));
    assert!(!output.contains("ActionInputType"));
    assert!(!output.contains("QueryInputType"));
    assert!(!output.contains("QueryOutputType"));
}
