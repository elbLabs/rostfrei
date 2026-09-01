#[test]
fn collects_tags_without_prescribing_behavior_signatures() {
    let mut item: syn::ItemTrait = syn::parse_quote! {
        pub trait FleetConsistency: Send {
            type Context;

            #[invariant(id = "unique-bicycles", label = "Unique bicycles")]
            fn unique<T>(&self, candidate: &T) -> bool where T: Eq {
                true
            }

            fn helper(&self) {}
        }
    };

    let invariants = super::invariant_collection::collect(&mut item.items).expect("tags");
    assert_eq!(invariants.len(), 1);
    assert_eq!(invariants[0].id.value(), "unique-bicycles");
}

#[test]
fn rejects_duplicate_invariant_ids() {
    let mut item: syn::ItemTrait = syn::parse_quote! {
        trait Contract {
            #[invariant(id = "same", label = "First")]
            fn first();
            #[invariant(id = "same", label = "Second")]
            fn second();
        }
    };

    assert!(super::invariant_collection::collect(&mut item.items).is_err());
}
