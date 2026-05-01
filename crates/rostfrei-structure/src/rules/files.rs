use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::SourceFileFacts;

use super::roles;

pub(super) fn check(
    root: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    for file in facts.values() {
        let file_name = file.path.file_name().and_then(|name| name.to_str());
        if file_name == Some("mod.rs") {
            check_composition(file, diagnostics);
        }

        roles::check_primary_placement(file, diagnostics);
        roles::check_implementation_placement(file, diagnostics);
        if file_name != Some("mod.rs") && !file.path.starts_with(&tests_root) {
            roles::check_counts(file, diagnostics);
        }

        if !file.path.starts_with(&tests_root) {
            check_test_placement(root, file, diagnostics);
        }
        for include_line in &file.include_lines {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::ModuleTopology,
                &file.path,
                *include_line,
                "include! is not supported in the typed domain tree",
            ));
        }
    }
}

fn check_composition(file: &SourceFileFacts, diagnostics: &mut Vec<Diagnostic>) {
    for (line, item) in &file.non_composition_items {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::ImpureModule,
            &file.path,
            *line,
            format!("{item} is not allowed in a composition-only mod.rs"),
        ));
    }
}

fn check_test_placement(root: &Path, file: &SourceFileFacts, diagnostics: &mut Vec<Diagnostic>) {
    let allowed_gateway_line = (file.path == root.join("mod.rs")).then(|| {
        file.modules
            .iter()
            .find(|module| module.name == "tests" && module.is_test_gate)
            .map(|module| module.line)
    });
    for test_line in &file.test_lines {
        if allowed_gateway_line.flatten() == Some(*test_line) {
            continue;
        }
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::TestPlacement,
                &file.path,
                *test_line,
                "domain tests must be placed below the sibling `tests/` tree",
            )
            .with_help("move this test to the mirrored path below `tests/`"),
        );
    }
}
