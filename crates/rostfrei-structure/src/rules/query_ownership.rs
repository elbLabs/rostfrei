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
        let query_path = directory.join("query.rs");
        let Some(query_file) = facts.get(&query_path) else {
            continue;
        };
        let Some(query) = unique_primary_name(query_file, PrimaryKind::Query) else {
            continue;
        };
        let Some(parent) = directory.parent() else {
            continue;
        };
        let Some(root) = aggregate_root(parent, facts, diagnostics) else {
            continue;
        };
        check_execute(directory, query, root, facts, diagnostics);
    }
}

fn check_execute(
    directory: &Path,
    query: &str,
    root: &str,
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
                "query directory requires `execute.rs`",
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
                "glob imports are not supported in query `execute.rs`",
            )
            .with_help("import the query trait and aggregate root type by name"),
        );
    }

    let implementations = execute
        .trait_implementations
        .iter()
        .filter(|implementation| implementation.trait_name.as_deref() == Some(query))
        .collect::<Vec<_>>();

    for implementation in &implementations {
        if !implementation.trait_is_direct || execute.aliases.iter().any(|alias| alias == query) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::InvalidStructure,
                    &execute_path,
                    implementation.line,
                    format!(
                        "query trait implementation must use direct unqualified, unaliased trait name `{query}`"
                    ),
                )
                .with_help(format!("use `impl {query} for {root}`")),
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
                "`execute.rs` must contain exactly one `{query}` implementation; found {}",
                implementations.len()
            ),
        ));
        return;
    };

    if implementation.trait_is_direct
        && !execute.aliases.iter().any(|alias| alias == query)
        && !matches!(
            &implementation.implementor,
            TypeReference::Direct(actual)
                if actual == root && !execute.aliases.iter().any(|alias| alias == actual)
        )
    {
        diagnostics.push(wrong_root(
            &execute_path,
            implementation,
            query,
            root,
            &execute.aliases,
        ));
    }
}

fn wrong_root(
    path: &Path,
    implementation: &TraitImplementation,
    query: &str,
    root: &str,
    aliases: &[String],
) -> Diagnostic {
    let actual = (!type_references_alias(&implementation.implementor, aliases))
        .then(|| type_display(&implementation.implementor))
        .flatten();
    let message = actual.map_or_else(
        || {
            format!(
                "`{query}` must be implemented for `{root}` using direct unqualified, unaliased type names"
            )
        },
        |actual| format!("`{query}` must be implemented for `{root}`; found `{actual}`"),
    );
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        path,
        implementation.line,
        message,
    )
    .with_help(format!("use `impl {query} for {root}`"))
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

fn aggregate_root<'a>(
    directory: &Path,
    facts: &'a BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a str> {
    let aggregate_path = directory.join("aggregate.rs");
    let aggregate = facts.get(&aggregate_path)?;
    let definitions = aggregate
        .trait_implementations
        .iter()
        .filter(|implementation| {
            implementation.trait_name.as_deref() == Some("AggregateDefinition")
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            1,
            "query ownership requires exactly one AggregateDefinition implementation",
        ));
        return None;
    };
    let [root] = definition.associated_root_types.as_slice() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            definition.line,
            "AggregateDefinition must declare exactly one associated `Root` type for query ownership",
        ));
        return None;
    };
    let Some(name) = root.name.as_deref() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            root.line,
            "AggregateDefinition::Root must be one direct unqualified type identifier for query ownership",
        ));
        return None;
    };
    if aggregate.aliases.iter().any(|alias| alias == name) {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            root.line,
            "AggregateDefinition::Root must use a direct unqualified, unaliased type identifier for query ownership",
        ));
        return None;
    }
    Some(name)
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
