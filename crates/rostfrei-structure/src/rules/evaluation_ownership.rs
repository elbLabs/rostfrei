use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{PrimaryKind, SourceFileFacts, TraitImplementation, TypeReference};

pub(super) fn check(
    root: &Path,
    directories: &[PathBuf],
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    for directory in directories
        .iter()
        .filter(|directory| !directory.starts_with(&tests_root))
    {
        for kind in [EvaluationKind::Decision, EvaluationKind::Invariant] {
            let declaration_path = directory.join(kind.declaration_file());
            let Some(declaration) = facts.get(&declaration_path) else {
                continue;
            };
            let Some(contract) = unique_primary_name(declaration, kind.primary()) else {
                continue;
            };
            let Some(parent) = directory.parent() else {
                continue;
            };
            let Some(owner) = expected_owner(parent, facts) else {
                continue;
            };
            check_evaluate(directory, contract, owner, kind, facts, diagnostics);
        }
    }
}

fn check_evaluate(
    directory: &Path,
    contract: &str,
    owner: &str,
    kind: EvaluationKind,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let evaluate_path = directory.join("evaluate.rs");
    let Some(evaluate) = facts.get(&evaluate_path) else {
        if !evaluate_path.is_file() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                evaluate_path,
                1,
                format!("{} directory requires `evaluate.rs`", kind.label()),
            ));
        }
        return;
    };

    for line in &evaluate.glob_import_lines {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                &evaluate_path,
                *line,
                format!(
                    "glob imports are not supported in {} `evaluate.rs`",
                    kind.label()
                ),
            )
            .with_help(format!(
                "import the {} trait and owner type by name",
                kind.label()
            )),
        );
    }

    let implementations = evaluate
        .trait_implementations
        .iter()
        .filter(|implementation| implementation.trait_name.as_deref() == Some(contract))
        .collect::<Vec<_>>();

    for implementation in &implementations {
        if !implementation.trait_is_direct || evaluate.aliases.iter().any(|alias| alias == contract)
        {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::InvalidStructure,
                    &evaluate_path,
                    implementation.line,
                    format!(
                        "{} trait implementation must use direct unqualified, unaliased trait name `{contract}`",
                        kind.label()
                    ),
                )
                .with_help(format!("use `impl {contract} for {owner}`")),
            );
        }
    }

    let [implementation] = implementations.as_slice() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidCardinality,
            evaluate_path,
            implementations
                .get(1)
                .map_or(1, |implementation| implementation.line),
            format!(
                "`evaluate.rs` must contain exactly one `{contract}` implementation; found {}",
                implementations.len()
            ),
        ));
        return;
    };

    if implementation.trait_is_direct
        && !evaluate.aliases.iter().any(|alias| alias == contract)
        && !owner_matches(&implementation.implementor, owner, &evaluate.aliases)
    {
        diagnostics.push(wrong_owner(
            &evaluate_path,
            implementation,
            contract,
            owner,
            &evaluate.aliases,
        ));
    }
}

fn wrong_owner(
    path: &Path,
    implementation: &TraitImplementation,
    contract: &str,
    owner: &str,
    aliases: &[String],
) -> Diagnostic {
    let actual = (!type_references_alias(&implementation.implementor, aliases))
        .then(|| type_display(&implementation.implementor))
        .flatten();
    let message = actual.map_or_else(
        || {
            format!(
                "`{contract}` must be implemented for `{owner}` using direct unqualified, unaliased type names"
            )
        },
        |actual| format!("`{contract}` must be implemented for `{owner}`; found `{actual}`"),
    );
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        path,
        implementation.line,
        message,
    )
    .with_help(format!("use `impl {contract} for {owner}`"))
}

fn expected_owner<'a>(
    directory: &Path,
    facts: &'a BTreeMap<PathBuf, SourceFileFacts>,
) -> Option<&'a str> {
    for (file, kind) in [
        ("aggregate.rs", PrimaryKind::Aggregate),
        ("entity.rs", PrimaryKind::Entity),
        ("value.rs", PrimaryKind::ValueObject),
    ] {
        if let Some(name) = facts
            .get(&directory.join(file))
            .and_then(|file| unique_primary_name(file, kind))
        {
            return Some(name);
        }
    }
    None
}

fn unique_primary_name(file: &SourceFileFacts, kind: PrimaryKind) -> Option<&str> {
    let declarations = file
        .top_level_items
        .iter()
        .filter(|item| item.primaries.contains(&kind))
        .collect::<Vec<_>>();
    let [declaration] = declarations.as_slice() else {
        return None;
    };
    declaration.name.as_deref()
}

fn owner_matches(reference: &TypeReference, owner: &str, aliases: &[String]) -> bool {
    matches!(reference, TypeReference::Direct(actual) if actual == owner && !aliases.contains(actual))
}

fn type_references_alias(reference: &TypeReference, aliases: &[String]) -> bool {
    match reference {
        TypeReference::Direct(name) => aliases.contains(name),
        TypeReference::SingleGeneric {
            constructor,
            argument,
        } => aliases.contains(constructor) || aliases.contains(argument),
        TypeReference::Unsupported => false,
    }
}

fn type_display(reference: &TypeReference) -> Option<String> {
    match reference {
        TypeReference::Direct(name) => Some(name.clone()),
        TypeReference::SingleGeneric {
            constructor,
            argument,
        } => Some(format!("{constructor}<{argument}>")),
        TypeReference::Unsupported => None,
    }
}

#[derive(Clone, Copy)]
enum EvaluationKind {
    Decision,
    Invariant,
}

impl EvaluationKind {
    const fn declaration_file(self) -> &'static str {
        match self {
            Self::Decision => "decision.rs",
            Self::Invariant => "contract.rs",
        }
    }

    const fn primary(self) -> PrimaryKind {
        match self {
            Self::Decision => PrimaryKind::Decision,
            Self::Invariant => PrimaryKind::Invariant,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Invariant => "invariant",
        }
    }
}
