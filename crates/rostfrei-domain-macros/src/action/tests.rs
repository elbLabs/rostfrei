use quote::quote;

fn arguments() -> proc_macro2::TokenStream {
    quote!(id = "rent-bicycle", label = "Rent bicycle")
}

fn expand(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let attributes = super::attributes::Attributes::parse(&arguments())?;
    super::validation::validate(&attributes)?;
    let item: syn::ItemTrait = syn::parse2(input)?;
    super::validation::validate_trait(&item)?;
    Ok(super::assembly::assemble(
        &syn::parse_quote!(::domain),
        item,
        &attributes,
    ))
}

#[test]
fn injects_semantic_metadata_into_the_authored_trait() {
    let input = quote! {
        pub trait RentBicycleAction {
            fn rent_bicycle(&mut self, bicycle: BicycleId) -> Result<(), RentalError>;
        }
    };
    let output = expand(input).expect("valid action").to_string();
    assert!(output.contains("pub trait RentBicycleAction"));
    assert!(output.contains("fn rent_bicycle"));
    assert!(output.contains("const LOCAL_ID"));
    assert!(output.contains("const LABEL"));
    assert!(output.contains("const DESCRIPTOR"));
    assert!(output.contains("rent-bicycle"));
    assert!(!output.contains("Owner"));
}

#[test]
fn preserves_visibility_generics_inheritance_and_signatures() {
    let output = expand(quote! {
        pub unsafe trait RentBicycleAction<'a, T>: Send + Parent<T>
        where
            T: 'a,
        {
            type Error;
            fn rent<P>(&mut self, input: &'a T, project: P) -> Result<(), Self::Error>
            where
                P: FnOnce(&T);
        }
    })
    .expect("valid action")
    .to_string();
    assert!(output.contains("pub unsafe trait RentBicycleAction < 'a , T > : Send + Parent < T >"));
    assert!(output.contains("where T : 'a"));
    assert!(output.contains("fn rent < P >"));
    assert!(output.contains("P : FnOnce"));
}

#[test]
fn rejects_non_trait_targets() {
    let error = super::expand::expand(
        &arguments(),
        quote! {
            pub struct RentBicycleAction;
        },
    )
    .expect_err("struct must be rejected");
    assert!(error.to_string().contains("may only be applied to a trait"));
}

#[test]
fn rejects_reserved_associated_item_collisions() {
    for item in [
        quote!(
            const LOCAL_ID: &'static str = "custom";
        ),
        quote!(
            fn LABEL();
        ),
        quote!(
            type DESCRIPTOR;
        ),
    ] {
        let input = quote! {
            trait RentBicycleAction {
                #item
            }
        };
        let error =
            super::expand::expand(&arguments(), input).expect_err("reserved item must be rejected");
        assert!(
            error
                .to_string()
                .contains("reserved domain_action associated item")
        );
    }
}

#[test]
fn rejects_removed_arguments() {
    for arguments in [
        quote!(id = "rent", label = "Rent", aggregate = Fleet),
        quote!(id = "rent", label = "Rent", entity = Bicycle),
        quote!(id = "rent", label = "Rent", domain_service = Service),
        quote!(id = "rent", label = "Rent", instance = Rent),
    ] {
        let error = super::expand::expand(
            &arguments,
            quote!(
                trait Action {}
            ),
        )
        .expect_err("removed argument must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported domain_action argument")
        );
    }
}

#[test]
fn validates_required_semantic_arguments() {
    let missing_id = super::expand::expand(
        &quote!(label = "Rent"),
        quote!(
            trait Action {}
        ),
    )
    .expect_err("id is required");
    assert!(missing_id.to_string().contains("missing id"));

    let duplicate_label = super::expand::expand(
        &quote!(id = "rent", label = "Rent", label = "Again"),
        quote!(
            trait Action {}
        ),
    )
    .expect_err("duplicate label must be rejected");
    assert!(duplicate_label.to_string().contains("duplicate label"));
}
