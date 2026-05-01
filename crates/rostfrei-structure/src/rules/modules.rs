use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{ModuleDeclaration, SourceFileFacts};

pub(super) fn check(
    root: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let directories = facts
        .keys()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect::<BTreeSet<_>>();

    for directory in directories {
        check_directory(root, &directory, facts, diagnostics);
    }
}

fn check_directory(
    root: &Path,
    directory: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mod_path = directory.join("mod.rs");
    let Some(mod_facts) = facts.get(&mod_path) else {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::ModuleTopology,
                directory,
                1,
                "a directory containing Rust domain source must have a mod.rs",
            )
            .with_help("add a composition-only `mod.rs`"),
        );
        return;
    };

    let mut declared = BTreeSet::new();
    for module in &mod_facts.modules {
        if reject_unsupported_module(root, directory, &mod_path, module, diagnostics) {
            continue;
        }
        register_module_target(
            directory,
            &mod_path,
            module,
            facts,
            &mut declared,
            diagnostics,
        );
    }

    for path in immediate_module_targets(directory, facts) {
        if !declared.contains(&path) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::ModuleTopology,
                    &path,
                    1,
                    "Rust source is not declared by mod.rs",
                )
                .with_help("add the corresponding `mod name;` declaration"),
            );
        }
    }
}

fn reject_unsupported_module(
    root: &Path,
    directory: &Path,
    mod_path: &Path,
    module: &ModuleDeclaration,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if module.is_inline {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::ModuleTopology,
            mod_path,
            module.line,
            format!("inline module `{}` is not supported", module.name),
        ));
        return true;
    }
    if module.has_path_override {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::ModuleTopology,
            mod_path,
            module.line,
            format!("#[path] is not supported for module `{}`", module.name),
        ));
        return true;
    }
    if module.is_test_gate && !(directory == root && module.name == "tests") {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::ModuleTopology,
            mod_path,
            module.line,
            "#[cfg(test)] is only allowed on the root `mod tests;` declaration",
        ));
    }
    false
}

fn register_module_target(
    directory: &Path,
    mod_path: &Path,
    module: &ModuleDeclaration,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
    declared: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let file_target = directory.join(format!("{}.rs", module.name));
    let directory_target = directory.join(&module.name).join("mod.rs");
    match (
        facts.contains_key(&file_target),
        facts.contains_key(&directory_target),
    ) {
        (false, false) => diagnostics.push(Diagnostic::new(
            DiagnosticCode::ModuleTopology,
            mod_path,
            module.line,
            format!("module `{}` has no matching source file", module.name),
        )),
        (true, false) => {
            declared.insert(file_target);
        }
        (false, true) => {
            declared.insert(directory_target);
        }
        (true, true) => diagnostics.push(Diagnostic::new(
            DiagnosticCode::ModuleTopology,
            mod_path,
            module.line,
            format!(
                "module `{}` is ambiguous: both file and directory forms exist",
                module.name
            ),
        )),
    }
}

fn immediate_module_targets(
    directory: &Path,
    facts: &BTreeMap<PathBuf, SourceFileFacts>,
) -> BTreeSet<PathBuf> {
    facts
        .keys()
        .filter_map(|path| {
            let parent = path.parent()?;
            if parent == directory && path.file_name().is_some_and(|name| name != "mod.rs") {
                return Some(path.clone());
            }
            if path.file_name().is_some_and(|name| name == "mod.rs")
                && parent.parent() == Some(directory)
            {
                return Some(path.clone());
            }
            None
        })
        .collect()
}
