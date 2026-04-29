#[test]
fn arbitrary_input_and_output_types_do_not_generate_dto_bounds() {
    let mut item: syn::ItemTrait = syn::parse_quote! {
        trait Actions {
            #[action(id = "calculate", label = "Calculate")]
            fn calculate(&self, input: ExternalInput) -> Vec<ExternalOutput>;
        }
    };
    let mut actions = super::trait_attributes::extract(&mut item.items).expect("action tags");
    super::trait_validation::validate(&item.items, &mut actions).expect("ordinary DTO signature");
    let output = super::trait_assembly::assemble(&syn::parse_quote!(::domain), item, &actions)
        .expect("action assembly")
        .to_string();

    assert!(output.contains("ExternalInput"));
    assert!(output.contains("ExternalOutput"));
    assert!(!output.contains("ActionInputType"));
    assert!(!output.contains("ActionOutputType"));
    assert!(!output.contains("raises"));
}

#[test]
fn authored_raises_metadata_is_rejected() {
    let mut item: syn::ItemTrait = syn::parse_quote! {
        trait Actions {
            #[action(id = "record", label = "Record", raises = [Recorded])]
            fn record(&mut self);
        }
    };

    assert!(super::trait_attributes::extract(&mut item.items).is_err());
}

#[test]
fn value_object_action_kind_is_rejected() {
    assert!(super::contract_arguments::parse(quote::quote!(value_object)).is_err());
}
