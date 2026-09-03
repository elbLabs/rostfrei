use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{PrimaryKind, SourceFileFacts};

pub(super) fn check_primary_placement(file: &SourceFileFacts, diagnostics: &mut Vec<Diagnostic>) {
    let file_name = file.path.file_name().and_then(|name| name.to_str());
    for primary in &file.primaries {
        if primary_is_placed(primary.kind, file_name) {
            continue;
        }
        let expected = primary
            .kind
            .expected_file()
            .unwrap_or("entity.rs or root.rs");
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::WrongPlacement,
                &file.path,
                primary.line,
                format!("{} must be declared in `{expected}`", primary.kind.label()),
            )
            .with_help(format!("move this declaration to `{expected}`")),
        );
    }
}

pub(super) fn check_implementation_placement(
    file: &SourceFileFacts,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let file_name = file.path.file_name().and_then(|name| name.to_str());
    for implementation in &file.trait_implementations {
        if implementation.trait_name.as_deref() == Some("DomainServiceDefinition") {
            if file_name != Some("service.rs") {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticCode::WrongPlacement,
                        &file.path,
                        implementation.line,
                        "DomainServiceDefinition implementation must be placed in `service.rs`",
                    )
                    .with_help("move this implementation beside its DomainService declaration"),
                );
            }
            continue;
        }
        if implementation.trait_name.as_deref() == Some("EntityDefinition") {
            if !matches!(file_name, Some("entity.rs" | "root.rs")) {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticCode::WrongPlacement,
                        &file.path,
                        implementation.line,
                        "EntityDefinition implementation must be placed in `entity.rs` or `root.rs`",
                    )
                    .with_help("move this implementation beside its Entity declaration"),
                );
            }
            continue;
        }
        let Some((trait_name, expected_file)) = implementation
            .trait_name
            .as_deref()
            .and_then(known_implementation_role)
        else {
            continue;
        };
        if file_name != Some(expected_file) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::WrongPlacement,
                    &file.path,
                    implementation.line,
                    format!("{trait_name} implementation must be placed in `{expected_file}`"),
                )
                .with_help(format!("move this implementation to `{expected_file}`")),
            );
        }
    }
}

pub(super) fn check_counts(file: &SourceFileFacts, diagnostics: &mut Vec<Diagnostic>) {
    let Some(file_name) = file.path.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    check_primary_count(file_name, file, diagnostics);
    check_implementation_count(file_name, file, diagnostics);
}

fn primary_is_placed(kind: PrimaryKind, file_name: Option<&str>) -> bool {
    match kind {
        PrimaryKind::Entity => matches!(file_name, Some("entity.rs" | "root.rs")),
        PrimaryKind::ValueObject => file_name == Some("value.rs"),
        _ => kind.expected_file() == file_name,
    }
}

fn known_implementation_role(trait_name: &str) -> Option<(&str, &'static str)> {
    match trait_name {
        "AggregateDefinition" => Some(("AggregateDefinition", "aggregate.rs")),
        "CommandHandler" => Some(("CommandHandler", "handler.rs")),
        "Apply" => Some(("Apply", "apply.rs")),
        "Initialize" => Some(("Initialize", "initialize.rs")),
        _ => None,
    }
}

fn check_primary_count(file_name: &str, file: &SourceFileFacts, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(kind) = role_kind(file_name) {
        let count = file
            .primaries
            .iter()
            .filter(|primary| primary.kind == kind)
            .count();
        if count != 1 {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidCardinality,
                &file.path,
                1,
                format!(
                    "`{file_name}` must contain exactly one {} declaration; found {count}",
                    kind.label()
                ),
            ));
        }
    }

    if file.primaries.len() > 1 {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidCardinality,
            &file.path,
            file.primaries.get(1).map_or(1, |primary| primary.line),
            format!(
                "a role file may contain only one primary Rostfrei declaration; found {}",
                file.primaries.len()
            ),
        ));
    }
}

fn check_implementation_count(
    file_name: &str,
    file: &SourceFileFacts,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let expected_trait = match file_name {
        "aggregate.rs" => Some("AggregateDefinition"),
        "service.rs" => Some("DomainServiceDefinition"),
        "entity.rs" | "root.rs" => Some("EntityDefinition"),
        "handler.rs" => Some("CommandHandler"),
        "apply.rs" => Some("Apply"),
        "initialize.rs" => Some("Initialize"),
        "evaluate.rs" => None,
        _ => return,
    };
    let definition_kind = match file_name {
        "entity.rs" | "root.rs" => Some(PrimaryKind::Entity),
        "service.rs" => Some(PrimaryKind::DomainService),
        _ => None,
    };
    let count = definition_kind.map_or_else(
        || {
            expected_trait.map_or(file.trait_implementations.len(), |expected_trait| {
                file.trait_implementations
                    .iter()
                    .filter(|implementation| {
                        implementation.trait_name.as_deref() == Some(expected_trait)
                    })
                    .count()
            })
        },
        |kind| {
            let primary = primary_name(file, kind);
            file.top_level_items
                .iter()
                .filter(|item| {
                    item.kind == crate::source::TopLevelItemKind::Implementation
                        && item.trait_name.as_deref() == expected_trait
                        && item.self_type.as_deref() == primary
                })
                .count()
        },
    );
    if count != 1 {
        let expected = expected_trait.unwrap_or("trait");
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidCardinality,
            &file.path,
            1,
            format!(
                "`{file_name}` must contain exactly one {expected} implementation; found {count}"
            ),
        ));
    }
}

fn primary_name(file: &SourceFileFacts, kind: PrimaryKind) -> Option<&str> {
    file.top_level_items
        .iter()
        .find(|item| item.primaries.contains(&kind))
        .and_then(|item| item.name.as_deref())
}

fn role_kind(file_name: &str) -> Option<PrimaryKind> {
    match file_name {
        "model.rs" => Some(PrimaryKind::Model),
        "context.rs" => Some(PrimaryKind::BoundedContext),
        "aggregate.rs" => Some(PrimaryKind::Aggregate),
        "event_set.rs" => Some(PrimaryKind::AggregateEvents),
        "entity.rs" | "root.rs" => Some(PrimaryKind::Entity),
        "identity.rs" => Some(PrimaryKind::Identity),
        "service.rs" => Some(PrimaryKind::DomainService),
        "value.rs" => Some(PrimaryKind::ValueObject),
        "action.rs" => Some(PrimaryKind::Action),
        "command.rs" => Some(PrimaryKind::Command),
        "event.rs" => Some(PrimaryKind::Event),
        "rejection.rs" => Some(PrimaryKind::Rejection),
        "decision.rs" => Some(PrimaryKind::Decision),
        "outcome.rs" => Some(PrimaryKind::DecisionOutcome),
        "query.rs" => Some(PrimaryKind::Query),
        "contract.rs" => Some(PrimaryKind::Invariant),
        "lifecycle.rs" => Some(PrimaryKind::Lifecycle),
        _ => None,
    }
}
