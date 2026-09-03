use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};

use super::role::DirectoryRole;

pub(super) fn missing_anchor(
    root: &Path,
    directory: &Path,
    roles: &BTreeMap<PathBuf, DirectoryRole>,
) -> Diagnostic {
    let expected = if directory.parent() == Some(root) {
        "`context.rs`".to_owned()
    } else {
        directory
            .parent()
            .and_then(|parent| roles.get(parent))
            .map_or_else(
                || "one supported role anchor".to_owned(),
                |role| expected_anchors(*role),
            )
    };
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        directory,
        1,
        format!("domain directory has no role anchor; expected exactly one of {expected}"),
    )
}

pub(super) fn multiple_anchors(directory: &Path, anchors: &[DirectoryRole]) -> Diagnostic {
    let actual = anchors
        .iter()
        .map(|role| format!("`{}` ({})", role.anchor(), role.label()))
        .collect::<Vec<_>>()
        .join(", ");
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        directory,
        1,
        format!("domain directory must have exactly one role anchor; found {actual}"),
    )
}

pub(super) fn wrong_parent(
    directory: &Path,
    role: DirectoryRole,
    parent: &Path,
    parent_role: Option<&DirectoryRole>,
) -> Diagnostic {
    let expected = parent_role.map_or_else(
        || "bounded context directory anchored by `context.rs`".to_owned(),
        |role| expected_children(*role),
    );
    let actual_parent = parent_role.map_or("domain root", |role| role.label());
    Diagnostic::new(
        DiagnosticCode::InvalidStructure,
        directory.join(role.anchor()),
        1,
        format!(
            "expected {expected} under {actual_parent} `{}`; found {} directory anchored by `{}`",
            parent.display(),
            role.label(),
            role.anchor()
        ),
    )
}

fn expected_children(parent: DirectoryRole) -> String {
    let allowed = parent.allowed_children();
    if allowed.is_empty() {
        return "no nested domain concept".to_owned();
    }
    allowed
        .iter()
        .map(|role| format!("{} (`{}`)", role.label(), role.anchor()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn expected_anchors(parent: DirectoryRole) -> String {
    let allowed = parent.allowed_children();
    if allowed.is_empty() {
        return format!("no nested concepts below a {} directory", parent.label());
    }
    allowed
        .iter()
        .map(|role| format!("`{}`", role.anchor()))
        .collect::<Vec<_>>()
        .join(", ")
}
