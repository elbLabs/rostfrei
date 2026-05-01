use std::{
    fs,
    path::{Path, PathBuf},
};

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
fn marker_only_identity_files_are_valid_role_declarations() {
    let domain_root = fixture_domain("valid_domain");
    let identity = domain_root.join("bike_rental/rental_fleet/identity.rs");
    let diagnostics = check_domain_root(&domain_root);

    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path != identity),
        "{diagnostics:#?}"
    );
}

#[test]
fn entity_definition_accessor_replaces_identity_and_value_object_field_tags() {
    let domain_root = fixture_domain("valid_domain");
    let root = fs::read_to_string(domain_root.join("bike_rental/rental_fleet/root.rs"))
        .expect("aggregate root fixture");
    let entity = fs::read_to_string(domain_root.join("bike_rental/rental_fleet/bicycle/entity.rs"))
        .expect("entity fixture");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    for source in [&root, &entity] {
        assert!(source.contains("fn identity(&self) -> &Self::Identity"));
        assert!(!source.contains("#[domain(identity)]"));
        assert!(!source.contains("#[domain(value_object)]"));
    }
}

#[test]
fn semantic_value_objects_and_plain_dto_companions_are_valid() {
    let domain_root = fixture_domain("valid_domain");
    let diagnostics = check_domain_root(&domain_root);
    let status =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/bicycle/status/value.rs"))
            .expect("semantic value object fixture");
    let input =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/rent_bicycle/input.rs"))
            .expect("action input DTO fixture");
    let output = fs::read_to_string(
        domain_root.join("bike_rental/rental_fleet/bicycle_availability/output.rs"),
    )
    .expect("query output DTO fixture");

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(status.contains("#[domain(id = \"bicycle-status\", label = \"Bicycle status\")]"));
    assert!(!status.contains("owner ="));
    assert!(!status.contains("actions ="));
    assert!(!input.contains("ValueObject"));
    assert!(!output.contains("ValueObject"));
}

#[test]
fn value_objects_are_modules_with_owned_behavior_and_mirrored_tests() {
    let domain_root = fixture_domain("valid_domain");
    let diagnostics = check_domain_root(&domain_root);
    let registration = "bike_rental/rental_fleet/bicycle/registration_number";

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    for path in [
        "value.rs",
        "normalize/action.rs",
        "normalize/execute.rs",
        "validity/contract.rs",
        "validity/evaluate.rs",
        "choose_format/decision.rs",
        "choose_format/outcome.rs",
        "choose_format/evaluate.rs",
    ] {
        assert!(
            domain_root.join(registration).join(path).is_file(),
            "missing value object fixture {registration}/{path}"
        );
    }
    for path in ["normalize.rs", "validity.rs", "choose_format.rs"] {
        assert!(
            domain_root
                .join("tests/bike_rental/rental_fleet/bicycle/registration_number")
                .join(path)
                .is_file(),
            "missing mirrored value object test {path}"
        );
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
            "bike_rental/rental_fleet/registration_number/qualified_decision/evaluate.rs",
            "`QualifiedDecision` must be implemented for `RegistrationNumber` using direct unqualified, unaliased type names",
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
fn action_roles_do_not_require_raises_metadata() {
    let domain_root = fixture_domain("valid_domain");
    let action =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/rent_bicycle/action.rs"))
            .expect("aggregate action fixture");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(action.contains("#[domain_action(id = \"rent-bicycle\", label = \"Rent bicycle\")]"));
    assert!(!action.contains("raises"));
}

#[test]
fn action_implementations_match_aggregate_entity_and_service_owners() {
    let domain_root = fixture_domain("valid_domain");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    for path in [
        "bike_rental/rental_fleet/rent_bicycle/execute.rs",
        "bike_rental/rental_fleet/bicycle/mark_rented/execute.rs",
        "bike_rental/fleet_planning/assess_demand/execute.rs",
    ] {
        assert!(domain_root.join(path).is_file(), "missing fixture {path}");
    }
}

#[test]
fn query_implementation_matches_the_enclosing_aggregate_root() {
    let domain_root = fixture_domain("valid_domain");
    let diagnostics = check_domain_root(&domain_root);
    let execute = domain_root.join("bike_rental/rental_fleet/bicycle_availability/execute.rs");

    assert!(execute.is_file(), "missing query execute fixture");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
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
fn decision_and_invariant_implementations_match_enclosing_owners() {
    let domain_root = fixture_domain("valid_domain");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    for path in [
        "bike_rental/rental_fleet/rental_assessment/evaluate.rs",
        "bike_rental/rental_fleet/fleet_consistency/evaluate.rs",
        "bike_rental/rental_fleet/bicycle/roadworthiness/evaluate.rs",
    ] {
        assert!(domain_root.join(path).is_file(), "missing fixture {path}");
    }
}

#[test]
fn invalid_evaluation_owner_contracts_emit_typed_diagnostics() {
    let domain_root = fixture_domain("evaluation_owner_contracts");
    let diagnostics = check_domain_root(&domain_root);
    let cases = [
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/missing_decision/evaluate.rs",
            "decision directory requires `evaluate.rs`",
        ),
        (
            DiagnosticCode::InvalidCardinality,
            "bike_rental/rental_fleet/wrong_decision_trait/evaluate.rs",
            "`evaluate.rs` must contain exactly one `WrongDecisionTrait` implementation; found 0",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/wrong_aggregate_decision/evaluate.rs",
            "`WrongAggregateDecision` must be implemented for `RentalFleetAggregate`; found `OtherAggregate`",
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
            "bike_rental/rental_fleet/qualified_decision_trait/evaluate.rs",
            "decision trait implementation must use direct unqualified, unaliased trait name `QualifiedDecisionTrait`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/qualified_invariant_owner/evaluate.rs",
            "`QualifiedInvariantOwner` must be implemented for `RentalFleetAggregate` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_decision_trait/evaluate.rs",
            "decision trait implementation must use direct unqualified, unaliased trait name `AliasDecisionTrait`",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/alias_invariant_owner/evaluate.rs",
            "`AliasInvariantOwner` must be implemented for `RentalFleetAggregate` using direct unqualified, unaliased type names",
        ),
        (
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/glob_decision/evaluate.rs",
            "glob imports are not supported in decision `evaluate.rs`",
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
fn command_roles_use_owner_independent_declarations() {
    let domain_root = fixture_domain("valid_domain");
    let command =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/rent_bicycle/command.rs"))
            .expect("command fixture");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(command.contains("#[domain(id = \"rent-bicycle\", label = \"Rent bicycle\")]"));
    assert!(!command.contains("owner ="));
    assert!(!command.contains("rejection ="));
    assert!(!command.contains("json"));
    assert!(!command.contains("runtime"));
}

#[test]
fn event_roles_use_owner_independent_declarations() {
    let domain_root = fixture_domain("valid_domain");
    let event =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/rent_bicycle/event.rs"))
            .expect("event fixture");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(event.contains("#[domain(id = \"bicycle-rented\", label = \"Bicycle rented\")]"));
    assert!(!event.contains("owner ="));
    assert!(!event.contains("schema_version = 1"));
}

#[test]
fn rejection_roles_use_owner_independent_declarations() {
    let domain_root = fixture_domain("valid_domain");
    let rejection =
        fs::read_to_string(domain_root.join("bike_rental/rental_fleet/rent_bicycle/rejection.rs"))
            .expect("domain rejection fixture");
    let diagnostics = check_domain_root(&domain_root);

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert!(rejection.contains("#[derive(DomainError)]"));
    assert!(rejection.contains("code = \"BICYCLE_UNAVAILABLE\""));
    assert!(rejection.contains("message = \"The bicycle is unavailable.\""));
    assert!(!rejection.contains("owner ="));
    assert!(!rejection.contains("json"));
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
            "hierarchy_lifecycle_under_aggregate",
            DiagnosticCode::InvalidStructure,
            "bike_rental/rental_fleet/rental_status/lifecycle.rs",
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
