use syn::DeriveInput;

#[test]
fn accepts_owner_independent_lifecycle_and_state_tags() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "rental-status", label = "Rental status")]
        enum RentalStatus {
            #[state(id = "available", label = "Available")]
            Available,
            #[state(id = "rented", label = "Rented")]
            Rented,
        }
    };
    let attributes = super::attributes::parse(&input.attrs).expect("attributes");
    let states =
        super::collection::collect(super::input::extract(&input).expect("input")).expect("states");
    let lifecycle = super::ir::Lifecycle {
        name: input.ident,
        id: attributes.id,
        label: attributes.label,
        states,
    };

    super::validation::validate(&lifecycle).expect("valid lifecycle tags");
    assert_eq!(lifecycle.states.len(), 2);
}

#[test]
fn rejects_removed_owner_and_initial_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "status", label = "Status", owner = Entity, initial = Open)]
        enum Status {
            #[state(id = "open", label = "Open")]
            Open,
        }
    };

    assert!(super::attributes::parse(&input.attrs).is_err());
}
