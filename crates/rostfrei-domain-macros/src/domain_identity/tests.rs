use quote::quote;
use syn::DeriveInput;

#[test]
fn accepts_arbitrary_non_generic_struct_and_enum_shapes() {
    let cases = [
        quote!(
            struct Unit;
        ),
        quote!(
            struct Tuple(uuid::Uuid, u32);
        ),
        quote!(
            struct Named {
                region: String,
                value: u64,
            }
        ),
        quote!(
            enum Composite {
                Numeric(u64),
                Text(String),
            }
        ),
    ];

    for tokens in cases {
        let input = syn::parse2::<DeriveInput>(tokens).expect("derive input");
        super::input::validate(&input).expect("supported identity shape");
    }
}

#[test]
fn rejects_generic_types_and_unions() {
    let cases = [
        quote!(
            struct Generic<T>(T);
        ),
        quote!(
            enum Generic<T> {
                Value(T),
            }
        ),
        quote!(union Bits { integer: u64, float: f64 }),
    ];

    for tokens in cases {
        let input = syn::parse2::<DeriveInput>(tokens).expect("derive input");
        assert!(super::input::validate(&input).is_err());
    }
}

#[test]
fn generates_only_the_public_marker_implementation() {
    let output = super::assembly::assemble(&syn::parse_quote!(::domain), &syn::parse_quote!(Id));
    let output = output.to_string();

    assert!(output.contains("impl :: domain :: DomainIdentity for Id"));
    assert!(!output.contains("DESCRIPTOR"));
    assert!(!output.contains("Owner"));
}
