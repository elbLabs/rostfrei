use syn::DeriveInput;

#[test]
fn accepts_arbitrary_named_fields_without_an_identity_tag() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "bicycle", label = "Bicycle")]
        struct Bicycle {
            id: BicycleId,
            details: BicycleDetails,
            active: bool,
        }
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    let fields =
        crate::field::extract(super::input::extract(&input).expect("input")).expect("fields");

    assert_eq!(attributes.id.value(), "bicycle");
    assert_eq!(fields.len(), 3);
    assert!(matches!(fields[0].role, crate::field::Role::Opaque));
    assert!(matches!(fields[1].role, crate::field::Role::Opaque));
}

#[test]
fn rejects_removed_entity_relationship_attributes() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "bicycle", label = "Bicycle", owner = Fleet)]
        struct Bicycle {
            id: BicycleId,
        }
    };

    assert!(super::attributes::Attributes::parse(&input.attrs).is_err());
}

#[test]
fn descriptor_uses_the_entity_id_without_hidden_identity_binding() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "bicycle", label = "Bicycle")]
        struct Bicycle {
            id: BicycleId,
            details: BicycleDetails,
        }
    };
    let attributes = super::attributes::Attributes::parse(&input.attrs).expect("attributes");
    let fields =
        crate::field::extract(super::input::extract(&input).expect("input")).expect("fields");
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &syn::parse_quote!(Bicycle),
        &attributes,
        &fields,
    )
    .to_string();

    assert!(output.contains("identity : :: domain :: DomainIdentityId { owner : id }"));
    assert!(!output.contains("IdentityDescriptor"));
    assert!(!output.contains("DomainIdentityType"));
}

#[test]
fn rejects_obsolete_identity_and_value_object_field_tags() {
    for role in [quote::quote!(identity), quote::quote!(value_object)] {
        let input: DeriveInput = syn::parse2(quote::quote! {
            #[domain(id = "bicycle", label = "Bicycle")]
            struct Bicycle {
                #[domain(#role)]
                value: BicycleId,
            }
        })
        .expect("derive input");
        let fields = super::input::extract(&input).expect("input");
        let error = crate::field::extract(fields)
            .err()
            .expect("obsolete role must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported field domain attribute")
        );
    }
}
