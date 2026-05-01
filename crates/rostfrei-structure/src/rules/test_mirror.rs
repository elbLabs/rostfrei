use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};

pub(super) fn check(
    root: &Path,
    directories: &[PathBuf],
    rust_files: &[PathBuf],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let tests_root = root.join("tests");
    let source_files = rust_files.iter().collect::<BTreeSet<_>>();
    let production_files = rust_files
        .iter()
        .filter(|path| !path.starts_with(&tests_root))
        .collect::<BTreeSet<_>>();

    let invalid_directories = check_directories(
        root,
        &tests_root,
        directories,
        &source_files,
        &production_files,
        diagnostics,
    );
    check_files(
        root,
        &tests_root,
        rust_files,
        &production_files,
        &invalid_directories,
        diagnostics,
    );
}

fn check_directories(
    root: &Path,
    tests_root: &Path,
    directories: &[PathBuf],
    source_files: &BTreeSet<&PathBuf>,
    production: &BTreeSet<&PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<PathBuf> {
    let mut invalid = BTreeSet::new();
    for test_directory in directories
        .iter()
        .filter(|path| path.starts_with(tests_root) && *path != tests_root)
    {
        let test_module = test_directory.join("mod.rs");
        if !source_files.contains(&test_module) {
            continue;
        }
        let Ok(relative) = test_directory.strip_prefix(tests_root) else {
            continue;
        };
        let expected = root.join(relative);
        let expected_module = expected.join("mod.rs");
        if !production.contains(&expected_module) {
            invalid.insert(test_directory.clone());
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::InvalidTestMirror,
                    test_module,
                    1,
                    "test directory does not mirror a domain directory",
                )
                .with_help(format!(
                    "expected a matching domain module at `{}`",
                    expected_module.display()
                )),
            );
        }
    }
    invalid
}

fn check_files(
    root: &Path,
    tests_root: &Path,
    rust_files: &[PathBuf],
    production: &BTreeSet<&PathBuf>,
    invalid_directories: &BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for test_file in rust_files
        .iter()
        .filter(|path| path.starts_with(tests_root))
    {
        if test_file.file_name().is_some_and(|name| name == "mod.rs") {
            continue;
        }
        if invalid_directories
            .iter()
            .any(|directory| test_file.starts_with(directory))
        {
            continue;
        }
        let Ok(relative) = test_file.strip_prefix(tests_root) else {
            continue;
        };
        let file_target = root.join(relative);
        let concept_target = concept_target(root, relative);
        let matching_targets = [
            production.contains(&file_target),
            concept_target
                .as_ref()
                .is_some_and(|target| production.contains(target)),
        ]
        .into_iter()
        .filter(|matches| *matches)
        .count();

        match matching_targets {
            1 => {}
            0 => diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::InvalidTestMirror,
                    test_file,
                    1,
                    "test source does not mirror a domain source file or concept directory",
                )
                .with_help(missing_target_help(&file_target, concept_target.as_deref())),
            ),
            _ => {
                let Some(concept_target) = concept_target else {
                    continue;
                };
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticCode::InvalidTestMirror,
                        test_file,
                        1,
                        "test source ambiguously mirrors both a domain source file and concept directory",
                    )
                    .with_help(format!(
                        "keep one matching target: `{}` or `{}`",
                        file_target.display(),
                        concept_target.display()
                    )),
                );
            }
        }
    }
}

fn concept_target(root: &Path, relative_test_file: &Path) -> Option<PathBuf> {
    let parent = relative_test_file.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative_test_file.file_stem()?;
    Some(root.join(parent).join(stem).join("mod.rs"))
}

fn missing_target_help(file_target: &Path, concept_target: Option<&Path>) -> String {
    concept_target.map_or_else(
        || format!("expected `{}`", file_target.display()),
        |concept_target| {
            format!(
                "expected `{}` or `{}`",
                file_target.display(),
                concept_target.display()
            )
        },
    )
}
