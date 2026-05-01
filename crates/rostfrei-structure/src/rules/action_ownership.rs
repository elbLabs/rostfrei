mod owner;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{PrimaryKind, SourceFileFacts, TraitImplementation};

use self::owner::{ExpectedOwner, expected_owner};

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
        let action_path = directory.join("action.rs");
        let Some(action_file) = facts.get(&action_path) else {
            continue;
        };
        let Some(action) = unique_primary_name(action_file, PrimaryKind::Action) else {
            continue;
        };
        let Some(parent) = directory.parent() else {
            continue;
        };
        let Some(owner) = expected_owner(parent, facts) else {
            continue;
        };
        check_execute(directory, action, &owner, facts, diagnostics);
    }
}

fn check_execute(
    directory: &Path,
    action: &str,
    owner: &ExpectedOwner,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let execute_path = directory.join("execute.rs");
    let Some(execute) = facts.get(&execute_path) else {
        if !execute_path.is_file() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                execute_path,
                1,
                "action directory requires `execute.rs`",
            ));
        }
        return;
    };
    for line in &execute.glob_import_lines {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                &execute_path,
                *line,
                "glob imports are not supported in action `execute.rs`",
            )
            .with_help("import the action trait and owner types by name"),
        );
    }
    let implementations = execute
        .trait_implementations
        .iter()
        .filter(|implementation| implementation.trait_name.as_deref() == Some(action))
        .collect::<Vec<_>>();

    for implementation in &implementations {
        if !implementation.trait_is_direct || execute.aliases.iter().any(|alias| alias == action) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::InvalidStructure,
                    &execute_path,
                    implementation.line,
                    format!(
                        "action trait implementation must use direct unqualified, unaliased trait name `{action}`"
                    ),
                )
                .with_help(format!("use `impl {action} for {}`", owner.display())),
            );
        }
    }

    let [implementation] = implementations.as_slice() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidCardinality,
            execute_path,
            implementations
                .get(1)
                .map_or(1, |implementation| implementation.line),
            format!(
                "`execute.rs` must contain exactly one `{action}` implementation; found {}",
                implementations.len()
            ),
        ));
        return;
    };
    if implementation.trait_is_direct
        && !execute.aliases.iter().any(|alias| alias == action)
        && !owner.matches(&implementation.implementor, &execute.aliases)
    {
        diagnostics.push(wrong_owner(
            &execute_path,
            implementation,
            action,
            owner,
            &execute.aliases,
        ));
    }
}

fn wrong_owner(
    path: &Path,
    implementation: &TraitImplementation,
    action: &str,
    owner: &ExpectedOwner,
    aliases: &[String],
) -> Diagnostic {
    let message = (!implementation.implementor.references_alias(aliases))
        .then(|| implementation.implementor.display())
        .flatten()
        .map_or_else(
        || {
            format!(
                "`{action}` must be implemented for `{}` using direct unqualified, unaliased type names",
                owner.display()
            )
        },
        |actual| {
            format!(
                "`{action}` must be implemented for `{}`; found `{actual}`",
                owner.display()
            )
        },
    );
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        path,
        implementation.line,
        message,
    )
    .with_help(format!("use `impl {action} for {}`", owner.display()))
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
