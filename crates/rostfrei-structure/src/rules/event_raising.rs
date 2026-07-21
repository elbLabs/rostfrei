use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::SourceFileFacts;

pub(super) fn check(
    root: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    approved_implementations: &BTreeMap<PathBuf, (usize, usize)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    for (path, file) in facts {
        if path.starts_with(&tests_root) {
            continue;
        }
        let approved = approved_implementations.get(path);
        for event_raise in &file.event_raises {
            if approved.is_some() && event_raise.implementation.as_ref() == approved {
                continue;
            }
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::EventRaisingOutsideAction,
                    path,
                    event_raise.line,
                    "event raising is only allowed inside domain action implementations",
                )
                .with_help("invoke the aggregate's domain action; raise events inside its validated `execute.rs` implementation"),
            );
        }
    }
}
