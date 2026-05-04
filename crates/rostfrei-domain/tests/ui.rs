#[test]
fn rejects_invalid_derives() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}

#[test]
fn checks_minimal_domain_action() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_action/supported.rs");
    cases.compile_fail("tests/ui/domain_action/invalid_metadata.rs");
    cases.compile_fail("tests/ui/domain_action/invalid_target.rs");
}

#[test]
fn checks_minimal_domain_query() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_query/supported.rs");
    cases.compile_fail("tests/ui/domain_query/invalid_metadata.rs");
    cases.compile_fail("tests/ui/domain_query/invalid_target.rs");
}

#[test]
fn checks_minimal_domain_decision() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_decision/supported.rs");
    cases.compile_fail("tests/ui/domain_decision/invalid_metadata.rs");
    cases.compile_fail("tests/ui/domain_decision/invalid_target.rs");
}

#[test]
fn checks_decision_outcomes() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/decision_outcome/duplicate_id.rs");
    cases.compile_fail("tests/ui/decision_outcome/invalid_target.rs");
    cases.compile_fail("tests/ui/decision_outcome/missing_metadata.rs");
}

#[test]
fn checks_minimal_domain_invariant() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_invariant/supported.rs");
    cases.compile_fail("tests/ui/domain_invariant/invalid_metadata.rs");
    cases.compile_fail("tests/ui/domain_invariant/invalid_target.rs");
}

#[test]
fn checks_domain_test_attributes() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/domain_test/existing_test.rs");
    cases.compile_fail("tests/ui/domain_test/invalid_signature.rs");
    cases.compile_fail("tests/ui/domain_test/non_function.rs");
    cases.compile_fail("tests/ui/domain_test/non_lifecycle.rs");
}

#[test]
fn checks_entity_lifecycle_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/entity_lifecycle/supported.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/non_enum.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/generic_enum.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/empty_enum.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/fieldful_variant.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/discriminant.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_enum_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_enum_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/invalid_lifecycle_id.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_state_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_state_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_state_id.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/blank_state_label.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_lifecycle_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_lifecycle_metadata.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/unknown_initial_state.rs");
}

#[test]
fn checks_state_transition_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/state_transition/supported.rs");
    cases.compile_fail("tests/ui/state_transition/non_enum.rs");
    cases.compile_fail("tests/ui/state_transition/missing_transition_metadata.rs");
    cases.compile_fail("tests/ui/state_transition/missing_edge_metadata.rs");
    cases.compile_fail("tests/ui/state_transition/duplicate_transition_id.rs");
}

#[test]
fn checks_semantic_scalars() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/semantic_scalar/field_provider.rs");
    cases.compile_fail("tests/ui/semantic_scalar/field_provider_type_mismatch.rs");
}
