use syn::DeriveInput;

fn parse(source: &str) -> syn::Result<Vec<super::input::EventVariant>> {
    let input = syn::parse_str::<DeriveInput>(source)?;
    super::input::extract(&input)
}

#[test]
fn accepts_plain_single_field_tuple_variants() {
    let variants = parse("enum Events { Created(Created), Renamed(events::Renamed) }")
        .expect("valid event set");

    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].name, "Created");
    assert_eq!(variants[1].name, "Renamed");
}

#[test]
fn rejects_unsupported_event_set_shapes() {
    let cases = [
        "struct Events;",
        "enum Events<T> { Created(T) }",
        "enum Events {}",
        "enum Events { Created }",
        "enum Events { Created { event: Created } }",
        "enum Events { Created(Created, Metadata) }",
        "enum Events { Created(Vec<Created>) }",
        "enum Events { First(Created), Second(Created) }",
    ];

    for source in cases {
        assert!(parse(source).is_err(), "unexpectedly accepted `{source}`");
    }
}
