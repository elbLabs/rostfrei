use syn::DeriveInput;

#[test]
fn accepts_state_transition_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        #[transition(state = RentalStatus)]
        enum RentalTransition {
            #[edge(
                id = "rent",
                label = "Rent",
                from = Available,
                to = Rented,
            )]
            Rent,
            #[edge(
                id = "return",
                label = "Return",
                from = Rented,
                to = Available,
            )]
            Return,
        }
    };
    let data = super::input::extract(&input).expect("input");
    let attributes = super::attributes::parse(&input.attrs).expect("attributes");
    let transitions = super::collection::collect(data).expect("transitions");
    let transition_set = super::ir::TransitionSet {
        name: input.ident,
        state: attributes.state,
        transitions,
    };

    super::validation::validate(&transition_set).expect("valid transition metadata");
    assert_eq!(transition_set.transitions.len(), 2);
}

#[test]
fn rejects_duplicate_transition_ids() {
    let input: DeriveInput = syn::parse_quote! {
        #[transition(state = RentalStatus)]
        enum RentalTransition {
            #[edge(id = "change", label = "Rent", from = Available, to = Rented)]
            Rent,
            #[edge(id = "change", label = "Return", from = Rented, to = Available)]
            Return,
        }
    };
    let data = super::input::extract(&input).expect("input");
    let attributes = super::attributes::parse(&input.attrs).expect("attributes");
    let transitions = super::collection::collect(data).expect("transitions");
    let transition_set = super::ir::TransitionSet {
        name: input.ident,
        state: attributes.state,
        transitions,
    };

    let error = super::validation::validate(&transition_set).expect_err("duplicate id");
    assert!(error.to_string().contains("duplicate state transition id"));
}

#[test]
fn assembles_static_descriptors_and_direct_match_arms() {
    let input: DeriveInput = syn::parse_quote! {
        #[transition(state = RentalStatus)]
        enum RentalTransition {
            #[edge(id = "rent", label = "Rent", from = Available, to = Rented)]
            Rent,
        }
    };

    let tokens = super::expand(input)
        .expect("expanded transition")
        .to_string();

    assert!(tokens.contains("const DESCRIPTORS"));
    assert!(tokens.contains("Self :: Rent => &"));
    assert!(tokens.contains("RentalStatus :: Available"));
    assert!(tokens.contains("RentalStatus :: Rented"));
}
