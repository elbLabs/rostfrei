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
fn checks_domain_test_attributes() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/domain_test/existing_test.rs");
    cases.compile_fail("tests/ui/domain_test/invalid_signature.rs");
    cases.compile_fail("tests/ui/domain_test/non_function.rs");
    cases.compile_fail("tests/ui/domain_test/unattached_decision.rs");
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
}

#[test]
fn checks_domain_decision_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_decision_contract/aggregate.rs");
    cases.pass("tests/ui/domain_decision_contract/borrowed_input.rs");
    cases.pass("tests/ui/domain_decision_contract/cfg.rs");
    cases.pass("tests/ui/domain_decision_contract/duplicate_impl.rs");
    cases.pass("tests/ui/domain_decision_contract/entity.rs");
    cases.pass("tests/ui/domain_decision_contract/public_cross_module.rs");
    cases.pass("tests/ui/domain_decision_contract/shadowed_result.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/bad_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/decision_outcome_duplicate_metadata.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/decision_outcome_invalid_target.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/decision_outcome_missing_metadata.rs");
    cases.pass("tests/ui/domain_decision_contract/decision_outcome_unsupported_payload.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decision_id.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/generic_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/non_trait_target.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/receiver.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/wrong_outcome_type.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/wrong_owner_kind_attachment.rs");
}

#[test]
fn checks_domain_invariant_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_invariant_contract/aggregate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/bad_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/blank_label.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_attribute.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_key.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/empty_trait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/generated_reference_collision.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/invalid_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_invariant_label.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/non_trait_target.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/reserved_descriptors.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/untagged_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsupported_invariant_metadata.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsupported_trait_item.rs");
}

#[test]
fn checks_semantic_scalars() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/semantic_scalar/field_provider.rs");
    cases.compile_fail("tests/ui/semantic_scalar/field_provider_type_mismatch.rs");
}
