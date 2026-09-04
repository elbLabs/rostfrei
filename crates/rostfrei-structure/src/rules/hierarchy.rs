mod diagnostic;
mod role;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::SourceFileFacts;

use self::diagnostic::{missing_anchor, multiple_anchors, wrong_parent};
use self::role::DirectoryRole;

pub(super) fn check(
    root: &Path,
    directories: &[PathBuf],
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    let production = directories
        .iter()
        .filter(|directory| *directory != root && !directory.starts_with(&tests_root));
    let mut roles = BTreeMap::new();

    for directory in production {
        let anchors = anchors_in(directory);
        match anchors.as_slice() {
            [role] => {
                roles.insert(directory.clone(), *role);
            }
            [] => diagnostics.push(missing_anchor(root, directory, &roles)),
            _ => diagnostics.push(multiple_anchors(directory, &anchors)),
        }
    }

    for (directory, role) in &roles {
        let Some(parent) = directory.parent() else {
            continue;
        };
        let parent_role = roles.get(parent);
        let allowed = if parent == root {
            &[DirectoryRole::BoundedContext][..]
        } else if let Some(parent_role) = parent_role {
            parent_role.allowed_children()
        } else {
            continue;
        };
        if !allowed.contains(role) {
            diagnostics.push(wrong_parent(directory, *role, parent, parent_role));
        }
    }

    check_aggregate_event_sets(&roles, facts, diagnostics);
    check_state_transition_companions(&roles, facts, diagnostics);
}

fn check_state_transition_companions(
    roles: &BTreeMap<PathBuf, DirectoryRole>,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (directory, role) in roles {
        let transition = directory.join("transition.rs");
        if *role != DirectoryRole::Lifecycle && facts.contains_key(&transition) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                transition,
                1,
                "`transition.rs` must be a companion in a lifecycle directory",
            ));
        }
    }
}

fn check_aggregate_event_sets(
    roles: &BTreeMap<PathBuf, DirectoryRole>,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (directory, role) in roles {
        let event_set = directory.join("event_set.rs");
        if *role == DirectoryRole::Aggregate {
            if facts.contains_key(&event_set) {
                check_aggregate_event_type(directory, &event_set, facts, diagnostics);
            } else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCode::InvalidStructure,
                    event_set,
                    1,
                    "aggregate directory requires `event_set.rs`",
                ));
            }
        } else if facts.contains_key(&event_set) {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                event_set,
                1,
                "`event_set.rs` must belong directly to an aggregate directory",
            ));
        }
    }
}

fn check_aggregate_event_type(
    directory: &Path,
    event_set_path: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let aggregate_path = directory.join("aggregate.rs");
    let Some(aggregate) = facts.get(&aggregate_path) else {
        return;
    };
    let definitions = aggregate
        .trait_implementations
        .iter()
        .filter(|implementation| {
            implementation.trait_name.as_deref() == Some("AggregateDefinition")
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return;
    };
    let [event_type] = definition.associated_event_types.as_slice() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            definition.line,
            "AggregateDefinition must declare exactly one associated `Event` type",
        ));
        return;
    };
    let Some(actual) = event_type.name.as_deref() else {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            aggregate_path,
            event_type.line,
            "AggregateDefinition::Event must be one direct unqualified type identifier",
        ));
        return;
    };
    let Some(expected) = facts.get(event_set_path).and_then(aggregate_event_set_name) else {
        return;
    };
    if actual != expected {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                aggregate_path,
                event_type.line,
                format!(
                    "AggregateDefinition::Event names `{actual}`, but `event_set.rs` declares `{expected}`"
                ),
            )
            .with_help(format!("use `type Event = {expected};`")),
        );
    }
}

fn aggregate_event_set_name(file: &SourceFileFacts) -> Option<&str> {
    let declarations = file
        .top_level_items
        .iter()
        .filter(|item| {
            item.nominal_shape == crate::source::NominalShape::Enum
                && item
                    .primaries
                    .contains(&crate::source::PrimaryKind::AggregateEvents)
        })
        .collect::<Vec<_>>();
    let [declaration] = declarations.as_slice() else {
        return None;
    };
    declaration.name.as_deref()
}

fn anchors_in(directory: &Path) -> Vec<DirectoryRole> {
    DirectoryRole::ALL
        .into_iter()
        .filter(|role| directory.join(role.anchor()).is_file())
        .collect()
}
