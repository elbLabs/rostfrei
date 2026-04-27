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
