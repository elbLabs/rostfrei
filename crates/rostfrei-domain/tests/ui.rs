#[test]
fn rejects_invalid_derives() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}

#[test]
fn rejects_invalid_action_extensions() {
    trybuild::TestCases::new().compile_fail("tests/ui/action_extension/*.rs");
}

#[test]
fn rejects_invalid_entity_actions() {
    trybuild::TestCases::new().compile_fail("tests/ui/entity_action/*.rs");
}

#[test]
fn rejects_invalid_aggregate_actions() {
    trybuild::TestCases::new().compile_fail("tests/ui/aggregate_action/*.rs");
}

#[test]
fn checks_domain_action_contract_arguments() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_action_contract/aggregate.rs");
    cases.pass("tests/ui/domain_action_contract/domain_service.rs");
    cases.pass("tests/ui/domain_action_contract/entity.rs");
    cases.pass("tests/ui/domain_action_contract/value_object.rs");
    cases.compile_fail("tests/ui/domain_action_contract/bare_argument.rs");
    cases.compile_fail("tests/ui/domain_action_contract/bare_on_struct.rs");
    cases.compile_fail("tests/ui/domain_action_contract/explicit_kind_on_impl.rs");
    cases.compile_fail("tests/ui/domain_action_contract/explicit_kind_on_struct.rs");
    cases.compile_fail("tests/ui/domain_action_contract/keyed_argument.rs");
    cases.compile_fail("tests/ui/domain_action_contract/legacy_group_on_impl.rs");
    cases.compile_fail("tests/ui/domain_action_contract/multiple_arguments.rs");
    cases.compile_fail("tests/ui/domain_action_contract/reserved_associated_item.rs");
    cases.compile_fail("tests/ui/domain_action_contract/unknown_kind.rs");
}

#[test]
fn checks_action_references() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/action_reference/generated_constants.rs");
    cases.pass("tests/ui/action_reference/owner_kinds.rs");
    cases.compile_fail("tests/ui/action_reference/generated_name_collision.rs");
    cases.compile_fail("tests/ui/action_reference/missing_implementation.rs");
    cases.compile_fail("tests/ui/action_reference/unknown_constant.rs");
    cases.compile_fail("tests/ui/action_reference/wrong_owner_type.rs");
}

#[test]
fn checks_domain_test_attributes() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/domain_test/malformed_reference.rs");
    cases.compile_fail("tests/ui/domain_test/existing_test.rs");
    cases.compile_fail("tests/ui/domain_test/invalid_signature.rs");
    cases.compile_fail("tests/ui/domain_test/non_function.rs");
    cases.compile_fail("tests/ui/domain_test/unknown_action_reference.rs");
    cases.compile_fail("tests/ui/domain_test/non_lifecycle.rs");
}

#[test]
fn checks_entity_lifecycle_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/entity_lifecycle/supported.rs");
    cases.pass("tests/ui/entity_lifecycle/module_qualified_action.rs");
    cases.pass("tests/ui/entity_lifecycle/self_transition_terminal.rs");
    cases.pass("tests/ui/entity_lifecycle/numeric_reference.rs");
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
    cases.compile_fail("tests/ui/entity_lifecycle/unknown_initial.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/unknown_target.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/malformed_action_reference.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/method_style_action_reference.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/generic_action_reference.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/hidden_action_reference.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_transition_action.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_transition_target.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_transition_action.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/unknown_action_reference.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_lexical_transition.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/duplicate_lifecycle_attachment.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/malformed_lifecycle_attachment.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/missing_action_implementation.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/unannotated_lifecycle.rs");
    cases.compile_fail("tests/ui/entity_lifecycle/lifecycle_owner_mismatch.rs");
}

#[test]
fn checks_domain_service_action_contracts() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/domain_service_action/non_public_trait.rs");
    cases.compile_fail("tests/ui/domain_service_action/receiver.rs");
    cases.compile_fail("tests/ui/domain_service_action/too_many_inputs.rs");
    cases.compile_fail("tests/ui/domain_service_action/bad_input_name.rs");
    cases.compile_fail("tests/ui/domain_service_action/bad_input_pattern.rs");
    cases.compile_fail("tests/ui/domain_service_action/wrong_command_owner.rs");
    cases.compile_fail("tests/ui/domain_service_action/wrong_error_owner.rs");
    cases.compile_fail("tests/ui/domain_service_action/direct_cross_context_event.rs");
    cases.compile_fail("tests/ui/domain_service_action/nested_cross_context_event.rs");
    cases.compile_fail("tests/ui/domain_service_action/missing_trait_implementation.rs");
    cases.compile_fail("tests/ui/domain_service_action/incomplete_trait_implementation.rs");
    cases.compile_fail("tests/ui/domain_service_action/unannotated_trait.rs");
    cases.compile_fail("tests/ui/domain_service_action/wrong_kind_attachment.rs");
    cases.compile_fail("tests/ui/domain_service_action/malformed_actions_list.rs");
    cases.compile_fail("tests/ui/domain_service_action/duplicate_action_path.rs");
    cases.compile_fail("tests/ui/domain_service_action/duplicate_actions_key.rs");
}

#[test]
fn checks_value_object_action_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/value_object_action/supported.rs");
    cases.compile_fail("tests/ui/value_object_action/unrestricted_public_trait.rs");
    cases.compile_fail("tests/ui/value_object_action/borrowed_receiver.rs");
    cases.compile_fail("tests/ui/value_object_action/mut_borrowed_receiver.rs");
    cases.compile_fail("tests/ui/value_object_action/typed_receiver.rs");
    cases.compile_fail("tests/ui/value_object_action/missing_constructor_input.rs");
    cases.compile_fail("tests/ui/value_object_action/excess_constructor_input.rs");
    cases.compile_fail("tests/ui/value_object_action/excess_transformation_input.rs");
    cases.compile_fail("tests/ui/value_object_action/scalar_output.rs");
    cases.compile_fail("tests/ui/value_object_action/unit_output.rs");
    cases.compile_fail("tests/ui/value_object_action/other_value_object_output.rs");
    cases.compile_fail("tests/ui/value_object_action/optional_self_output.rs");
    cases.compile_fail("tests/ui/value_object_action/domain_event_output.rs");
    cases.compile_fail("tests/ui/value_object_action/wrong_error_owner.rs");
    cases.compile_fail("tests/ui/value_object_action/missing_trait_implementation.rs");
    cases.compile_fail("tests/ui/value_object_action/incomplete_trait_implementation.rs");
    cases.compile_fail("tests/ui/value_object_action/unannotated_trait.rs");
    cases.compile_fail("tests/ui/value_object_action/wrong_kind_attachment.rs");
    cases.compile_fail("tests/ui/value_object_action/malformed_actions_list.rs");
    cases.compile_fail("tests/ui/value_object_action/duplicate_action_path.rs");
    cases.compile_fail("tests/ui/value_object_action/duplicate_actions_key.rs");
}

#[test]
fn checks_domain_decision_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_decision_contract/aggregate.rs");
    cases.pass("tests/ui/domain_decision_contract/domain_service.rs");
    cases.pass("tests/ui/domain_decision_contract/entity.rs");
    cases.pass("tests/ui/domain_decision_contract/value_object.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/async_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/auto_trait.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/bad_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/blank_label.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/borrowed_input.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/default_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/destructured_input.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decision_attribute.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decision_id.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decision_key.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decision_path.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/duplicate_decisions_key.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/empty_trait.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/existing_supertrait.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/extern_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/generic_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/generated_reference_collision.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/incomplete_implementation.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/invalid_decision_id.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/malformed_decisions_list.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/method_where_clause.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/misnamed_input.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_decision_id.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_decision_label.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_implementation.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/missing_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/multiple_inputs.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/multiple_macro_kinds.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/non_trait_target.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/option_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/raw_scalar_input.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/raw_scalar_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/receiver.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/reserved_associated_item.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/result_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/trait_generics.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/trait_where_clause.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unannotated_attachment.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unit_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unsafe_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unsafe_trait.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unsupported_decision_metadata.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/unsupported_trait_item.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/variadic_method.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/vec_output.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/wrong_owner_kind_attachment.rs");
    cases.compile_fail("tests/ui/domain_decision_contract/zero_inputs.rs");
}

#[test]
fn checks_domain_invariant_contracts() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/domain_invariant_contract/aggregate.rs");
    cases.pass("tests/ui/domain_invariant_contract/entity.rs");
    cases.pass("tests/ui/domain_invariant_contract/shadowed_prelude.rs");
    cases.pass("tests/ui/domain_invariant_contract/value_object.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/async_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/auto_trait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/bad_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/blank_label.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/by_value_candidate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/default_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/destructured_candidate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/domain_service_attachment.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/domain_service_kind.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_attribute.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_key.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariant_path.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/duplicate_invariants_key.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/empty_trait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/existing_supertrait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/explicit_lifetime_candidate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/extern_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/generic_invariant_path.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/generated_reference_collision.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/generic_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/incomplete_implementation.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/invalid_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/keyed_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/legacy_group_on_impl.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/malformed_invariants_list.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/method_where_clause.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/misnamed_candidate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_implementation.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_invariant_id.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_invariant_label.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_macro_kind.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/missing_output.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/multiple_candidates.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/multiple_macro_kinds.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/mutable_candidate.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/non_trait_target.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/qualified_self_invariant_path.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/receiver.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/removed_invariant_groups.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/reserved_append_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/reserved_descriptors.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/reserved_requirement.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/scalar_output.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/trait_generics.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/trait_where_clause.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unannotated_attachment.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/untagged_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unrestricted_public_trait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsafe_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsafe_trait.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsupported_invariant_metadata.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/unsupported_trait_item.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/variadic_method.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/wrong_candidate_type.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/wrong_owner_kind_attachment.rs");
    cases.compile_fail("tests/ui/domain_invariant_contract/zero_candidates.rs");
}

#[test]
fn checks_tagged_value_objects() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/value_object_tagged/supported.rs");
    cases.compile_fail("tests/ui/value_object_tagged/entity_payload.rs");
    cases.compile_fail("tests/ui/value_object_tagged/custom_unannotated_type.rs");
    cases.compile_fail("tests/ui/value_object_tagged/wrong_identity_type.rs");
    cases.compile_fail("tests/ui/value_object_tagged/wrong_value_object_type.rs");
    cases.compile_fail("tests/ui/value_object_tagged/semantic_scalar_provider_mismatch.rs");
    cases.compile_fail("tests/ui/value_object_tagged/conflicting_roles.rs");
    cases.compile_fail("tests/ui/value_object_tagged/metadata_precedes_payload.rs");
}

#[test]
fn checks_semantic_scalars() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/semantic_scalar/field_provider.rs");
    cases.pass("tests/ui/semantic_scalar/identity_provider.rs");
    cases.pass("tests/ui/semantic_scalar/canonical_identity.rs");
    cases.compile_fail("tests/ui/semantic_scalar/missing_provider.rs");
    cases.compile_fail("tests/ui/semantic_scalar/malformed_provider.rs");
    cases.compile_fail("tests/ui/semantic_scalar/generic_provider.rs");
    cases.compile_fail("tests/ui/semantic_scalar/duplicate_conflicting_roles.rs");
    cases.compile_fail("tests/ui/semantic_scalar/identity_field_attribute.rs");
    cases.compile_fail("tests/ui/semantic_scalar/field_provider_type_mismatch.rs");
    cases.compile_fail("tests/ui/semantic_scalar/identity_provider_type_mismatch.rs");
}

#[test]
fn rejects_invalid_queries() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/query_invalid.rs");
    cases.compile_fail("tests/ui/query_metadata.rs");
    cases.compile_fail("tests/ui/query_type_contracts.rs");
}
