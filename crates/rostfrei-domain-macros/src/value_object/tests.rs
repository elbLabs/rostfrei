use quote::quote;
use syn::DeriveInput;

#[test]
fn accepts_arbitrary_non_generic_struct_and_enum_shapes() {
    let cases = [
        quote!(
            struct Money(Decimal);
        ),
        quote!(
            struct Address {
                lines: Vec<ExternalLine>,
            }
        ),
        quote!(
            enum Status {
                Open,
                Closed { reason: ExternalReason },
            }
        ),
    ];
    for tokens in cases {
        let input = syn::parse2::<DeriveInput>(tokens).expect("derive input");
        super::input::validate(&input).expect("supported value object shape");
    }
}

#[test]
fn rejects_removed_owner_and_action_metadata() {
    let input: DeriveInput = syn::parse_quote! {
        #[domain(id = "money", label = "Money", owner = Account, actions = [MoneyActions])]
        struct Money;
    };

    assert!(super::attributes::Attributes::parse(&input.attrs).is_err());
}

#[test]
fn generates_only_the_semantic_value_object_contract() {
    let attributes = super::attributes::Attributes {
        id: syn::LitStr::new("money", proc_macro2::Span::call_site()),
        label: syn::LitStr::new("Money", proc_macro2::Span::call_site()),
    };
    let output = super::assembly::assemble(
        &syn::parse_quote!(::domain),
        &syn::parse_quote!(Money),
        &attributes,
    )
    .to_string();

    assert!(output.contains("impl :: domain :: ValueObject for Money"));
    assert!(!output.contains("DecisionInputType"));
    assert!(!output.contains("DecisionOutcomeValueType"));
    assert!(!output.contains("DomainErrorOwnerType"));
    assert!(!output.contains("ActionInputType"));
    assert!(!output.contains("QueryInputType"));
    assert!(!output.contains("shape"));
}

#[test]
fn unannotated_custom_fields_are_opaque() {
    let input: DeriveInput = syn::parse_quote!(
        struct Wrapper {
            value: ExternalDto,
        }
    );
    let syn::Data::Struct(data) = input.data else {
        panic!("struct fixture");
    };
    let fields = crate::field::extract(&data.fields).expect("opaque field");

    assert!(matches!(fields[0].role, crate::field::Role::Opaque));
}
