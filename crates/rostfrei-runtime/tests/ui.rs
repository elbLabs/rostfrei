#[test]
fn event_serialization_contract() {
    // Keep Serde's candidate-implementation diagnostics independent of the
    // facade's optional adapters and their additional Serde implementations.
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
