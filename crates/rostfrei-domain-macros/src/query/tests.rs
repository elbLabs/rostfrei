use quote::quote;

fn arguments() -> proc_macro2::TokenStream {
    quote!(id = "available-bicycles", label = "Available bicycles")
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
fn injects_only_owner_independent_semantic_metadata() {
    let output = expand(quote! {
        pub trait AvailableBicyclesQuery {
            fn available_bicycles(&self, station: StationId) -> Vec<BicycleView>;
        }
    })
    .expect("valid query")
    .to_string();

    assert!(output.contains("pub trait AvailableBicyclesQuery"));
    assert!(output.contains("fn available_bicycles"));
    assert!(output.contains("const LOCAL_ID"));
    assert!(output.contains("const LABEL"));
    assert!(output.contains("const DESCRIPTOR"));
    assert!(output.contains("QueryId"));
    assert!(!output.contains("Owner"));
    assert!(!output.contains("Group"));
}

#[test]
fn preserves_visibility_generics_inheritance_and_signatures() {
    let output = expand(quote! {
        pub unsafe trait SearchQuery<'a, T>: Send + Parent<T>
        where
            T: 'a,
        {
            type View;
            fn search<P>(&self, input: &'a T, project: P) -> Result<Self::View, QueryError>
            where
                P: FnOnce(&T);
        }
    })
    .expect("valid query")
    .to_string();

    assert!(output.contains("pub unsafe trait SearchQuery < 'a , T > : Send + Parent < T >"));
    assert!(output.contains("where T : 'a"));
    assert!(output.contains("fn search < P >"));
    assert!(output.contains("P : FnOnce"));
}

#[test]
fn rejects_non_trait_targets() {
    let error = super::expand::expand(
        &arguments(),
        quote! {
            pub struct AvailableBicyclesQuery;
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
        let error = super::expand::expand(
            &arguments(),
            quote! {
                trait Query {
                    #item
                }
            },
        )
        .expect_err("reserved item must be rejected");
        assert!(
            error
                .to_string()
                .contains("reserved domain_query associated item")
        );
    }
}

#[test]
fn rejects_removed_owner_and_group_arguments() {
    for arguments in [
        quote!(id = "query", label = "Query", owner = Fleet),
        quote!(id = "query", label = "Query", group = Queries),
    ] {
        let error = super::expand::expand(
            &arguments,
            quote! {
                trait Query {}
            },
        )
        .expect_err("removed argument must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported domain_query argument")
        );
    }
}
