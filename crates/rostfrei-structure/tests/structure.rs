use std::path::{Path, PathBuf};

use rostfrei_structure::{DiagnosticCode, check_domain_root};

fn fixture_domain(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
        .join("src")
        .join("domain")
}

#[test]
fn agreed_domain_structure_is_valid() {
    for fixture in ["valid_domain", "content_valid_role_impls"] {
        let domain_root = fixture_domain(fixture);
        let diagnostics = check_domain_root(&domain_root);

        assert!(diagnostics.is_empty(), "{fixture}: {diagnostics:#?}");
    }
}

#[test]
fn invalid_value_object_module_and_behavior_conventions_are_typed() {
    let domain_root = fixture_domain("value_object_conventions");
    let diagnostics = check_domain_root(&domain_root);
    let cases = [
        (
            DiagnosticCode::WrongPlacement,
            "bike_rental/rental_fleet/status.rs",
            "ValueObject must be declared in `value.rs`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/fleet_planning/invalid_value/value.rs",
            "found value object directory anchored by `value.rs`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/registration_number/invalid_query/query.rs",
            "found query directory anchored by `query.rs`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/registration_number/missing_action/execute.rs",
            "action directory requires `execute.rs`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/registration_number/wrong_action/execute.rs",
            "`WrongAction` must be implemented for `RegistrationNumber`; found `OtherValue`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/registration_number/qualified_policy/evaluate.rs",
            "`QualifiedPolicy` must be implemented for `RegistrationNumber` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/registration_number/duplicate_invariant/evaluate.rs",
            "`evaluate.rs` must contain exactly one `DuplicateInvariant` implementation; found 2",
        ),
        (
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/impure_value/value.rs",
            "implementation is not allowed in `value.rs`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/missing_value/value.rs",
            "`value.rs` must contain exactly one ValueObject declaration; found 0",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/duplicate_value/value.rs",
            "`value.rs` must contain exactly one ValueObject declaration; found 2",
        ),
    ];

    for (code, path, message) in cases {
        let expected_path = domain_root.join(path);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.path == expected_path
                    && diagnostic.message.contains(message)
            }),
            "missing {code} at {path} containing `{message}`: {diagnostics:#?}"
        );
    }
}

#[test]
fn invalid_query_owner_contracts_emit_typed_diagnostics() {
    let domain_root = fixture_domain("query_owner_contracts");
    let diagnostics = check_domain_root(&domain_root);
    let cases = [
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/missing_execute/execute.rs",
            "query directory requires `execute.rs`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/wrong_trait/execute.rs",
            "`execute.rs` must contain exactly one `WrongTraitQuery` implementation; found 0",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/wrong_root/execute.rs",
            "`WrongRootQuery` must be implemented for `RentalFleet`; found `OtherRoot`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/duplicate_impl/execute.rs",
            "`execute.rs` must contain exactly one `DuplicateImplQuery` implementation; found 2",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_trait/execute.rs",
            "query trait implementation must use direct unqualified, unaliased trait name `QualifiedTraitQuery`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_root/execute.rs",
            "`QualifiedRootQuery` must be implemented for `RentalFleet` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_trait/execute.rs",
            "query trait implementation must use direct unqualified, unaliased trait name `AliasTraitQuery`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_root/execute.rs",
            "`AliasRootQuery` must be implemented for `RentalFleet` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/glob_import/execute.rs",
            "glob imports are not supported in query `execute.rs`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/qualified_definition/aggregate.rs",
            "AggregateDefinition::Root must be one direct unqualified type identifier for query ownership",
        ),
    ];

    for (code, path, message) in cases {
        let expected_path = domain_root.join(path);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.path == expected_path
                    && diagnostic.message == message
            }),
            "missing {code} at {path} with `{message}`: {diagnostics:#?}"
        );
    }
}

#[test]
fn invalid_evaluation_owner_contracts_emit_typed_diagnostics() {
    let domain_root = fixture_domain("evaluation_owner_contracts");
    let diagnostics = check_domain_root(&domain_root);
    let cases = [
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/missing_policy/evaluate.rs",
            "policy directory requires `evaluate.rs`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/wrong_policy_trait/evaluate.rs",
            "`evaluate.rs` must contain exactly one `WrongPolicyTrait` implementation; found 0",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/wrong_aggregate_policy/evaluate.rs",
            "`WrongAggregatePolicy` must be implemented for `RentalFleetAggregate`; found `OtherAggregate`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/fleet_planning/wrong_service_policy/evaluate.rs",
            "`WrongServicePolicy` must be implemented for `FleetPlanning`; found `OtherService`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/bicycle/wrong_entity_invariant/evaluate.rs",
            "`WrongEntityInvariant` must be implemented for `Bicycle`; found `OtherBicycle`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/duplicate_invariant/evaluate.rs",
            "`evaluate.rs` must contain exactly one `DuplicateInvariant` implementation; found 2",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_policy_trait/evaluate.rs",
            "policy trait implementation must use direct unqualified, unaliased trait name `QualifiedPolicyTrait`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_invariant_owner/evaluate.rs",
            "`QualifiedInvariantOwner` must be implemented for `RentalFleetAggregate` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_policy_trait/evaluate.rs",
            "policy trait implementation must use direct unqualified, unaliased trait name `AliasPolicyTrait`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_invariant_owner/evaluate.rs",
            "`AliasInvariantOwner` must be implemented for `RentalFleetAggregate` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/glob_policy/evaluate.rs",
            "glob imports are not supported in policy `evaluate.rs`",
        ),
    ];

    for (code, path, message) in cases {
        let expected_path = domain_root.join(path);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.path == expected_path
                    && diagnostic.message == message
            }),
            "missing {code} at {path} with `{message}`: {diagnostics:#?}"
        );
    }
}

#[test]
fn invalid_action_owner_contracts_emit_typed_diagnostics() {
    let domain_root = fixture_domain("action_owner_contracts");
    let diagnostics = check_domain_root(&domain_root);
    let cases = [
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/missing_execute/execute.rs",
            "action directory requires `execute.rs`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/wrong_trait/execute.rs",
            "`execute.rs` must contain exactly one `WrongTraitAction` implementation; found 0",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/wrong_aggregate/execute.rs",
            "`WrongAggregateAction` must be implemented for `AggregateInstance<RentalFleetAggregate>`; found `AggregateInstance<OtherAggregate>`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/bicycle/wrong_entity/execute.rs",
            "`WrongEntityAction` must be implemented for `Bicycle`; found `OtherBicycle`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/fleet_planning/wrong_service/execute.rs",
            "`WrongServiceAction` must be implemented for `FleetPlanning`; found `OtherService`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/duplicate_impl/execute.rs",
            "`execute.rs` must contain exactly one `DuplicateImplAction` implementation; found 2",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_trait/execute.rs",
            "action trait implementation must use direct unqualified, unaliased trait name `QualifiedTraitAction`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_owner/execute.rs",
            "`QualifiedOwnerAction` must be implemented for `AggregateInstance<RentalFleetAggregate>` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/glob_import/execute.rs",
            "glob imports are not supported in action `execute.rs`",
        ),
    ];

    for (code, path, message) in cases {
        let expected_path = domain_root.join(path);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == code
                    && diagnostic.path == expected_path
                    && diagnostic.message == message
            }),
            "missing {code} at {path} with `{message}`: {diagnostics:#?}"
        );
    }
}

#[test]
fn non_module_test_asset_directories_are_ignored() {
    let domain_root = fixture_domain("test_mirror_valid");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn invalid_test_mirrors_emit_rf008() {
    assert_invalid_cases(&[
        (
            "test_mirror_unknown_concept",
            DiagnosticCode::InvalidTestMirror,
            "tests/bike_rental/rental_fleet/return_bicycle.rs",
        ),
        (
            "test_mirror_wrong_depth",
            DiagnosticCode::InvalidTestMirror,
            "tests/bike_rental/rent_bicycle.rs",
        ),
        (
            "test_mirror_unknown_directory",
            DiagnosticCode::InvalidTestMirror,
            "tests/bike_rental/rental_fleet/retired_capabilities/mod.rs",
        ),
        (
            "test_mirror_missing_source",
            DiagnosticCode::InvalidTestMirror,
            "tests/bike_rental/rental_fleet/initialize.rs",
        ),
    ]);
}

#[test]
fn invalid_structures_emit_the_expected_diagnostic() {
    assert_invalid_cases(&[
        (
            "logic_in_mod",
            DiagnosticCode::ImpureModule,
            "bike_rental/rental_fleet/mod.rs",
        ),
        (
            "macro_in_wrong_file",
            DiagnosticCode::WrongPlacement,
            "bike_rental/rental_fleet/rent_bicycle/command.rs",
        ),
        (
            "multiple_primary_declarations",
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/rent_bicycle/event.rs",
        ),
        (
            "test_outside_tests",
            DiagnosticCode::TestPlacement,
            "bike_rental/rental_fleet/rent_bicycle/execute.rs",
        ),
        (
            "undeclared_file",
            DiagnosticCode::ModuleTopology,
            "bike_rental/rental_fleet/orphan.rs",
        ),
        (
            "missing_module_file",
            DiagnosticCode::ModuleTopology,
            "bike_rental/rental_fleet/mod.rs",
        ),
        (
            "hierarchy_nested_bounded_context",
            DiagnosticCode::InvalidStructure,
            "outer_context/bike_rental/context.rs",
        ),
        (
            "hierarchy_nested_aggregate",
            DiagnosticCode::InvalidStructure,
            "bike_rental/operations/rental_fleet/aggregate.rs",
        ),
        (
            "hierarchy_entity_under_action",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/rent_bicycle/bicycle/entity.rs",
        ),
        (
            "hierarchy_policy_under_action",
            DiagnosticCode::InvalidStructure,
            "bike_rental/fleet_planning/rebalance_fleet/scheduling/policy.rs",
        ),
        (
            "hierarchy_transition_under_aggregate",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/transition.rs",
        ),
        (
            "hierarchy_missing_anchor",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/support",
        ),
        (
            "hierarchy_multiple_anchors",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/rental_assessment",
        ),
    ]);
}

#[test]
fn aggregate_definition_and_event_set_conventions_are_enforced() {
    assert_invalid_cases(&[
        (
            "aggregate_missing_event_set",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/event_set.rs",
        ),
        (
            "aggregate_missing_definition",
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/aggregate.rs",
        ),
        (
            "event_set_not_enum",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/event_set.rs",
        ),
        (
            "aggregate_event_type_invalid",
            DiagnosticCode::InvalidStructure,
            "bike_rental/mismatch/aggregate.rs",
        ),
        (
            "aggregate_event_type_invalid",
            DiagnosticCode::InvalidStructure,
            "bike_rental/qualified/aggregate.rs",
        ),
    ]);
}

#[test]
fn entity_definition_conventions_are_enforced() {
    assert_invalid_cases(&[
        (
            "entity_missing_definition",
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/root.rs",
        ),
        (
            "entity_wrong_definition_target",
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/root.rs",
        ),
    ]);
}

#[test]
fn domain_service_definition_conventions_are_enforced() {
    assert_invalid_cases(&[(
        "domain_service_missing_definition",
        DiagnosticCode::InvalidCardinality,
        "bike_rental/fleet_planning/service.rs",
    )]);
}

#[test]
fn impure_role_files_emit_rf007() {
    assert_invalid_cases(&[
        (
            "content_free_function_event",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/event.rs",
        ),
        (
            "content_public_helper_execute",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/execute.rs",
        ),
        (
            "content_unrelated_const_context",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/context.rs",
        ),
        (
            "content_unrelated_impl_identity",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/identity.rs",
        ),
        (
            "content_unrelated_macro_command",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/command.rs",
        ),
        (
            "content_unrelated_static_event",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/event.rs",
        ),
        (
            "content_unrelated_struct_command",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/command.rs",
        ),
        (
            "content_unrelated_trait_action",
            DiagnosticCode::UnexpectedRoleContent,
            "bike_rental/rental_fleet/rent_bicycle/action.rs",
        ),
    ]);
}

fn assert_invalid_cases(cases: &[(&str, DiagnosticCode, &str)]) {
    for &(fixture, code, relative_path) in cases {
        let domain_root = fixture_domain(fixture);
        let expected_path = domain_root.join(relative_path);
        let diagnostics = check_domain_root(&domain_root);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code && diagnostic.path == expected_path),
            "fixture `{fixture}` did not emit {code} at {relative_path}: {diagnostics:#?}"
        );
    }
}
