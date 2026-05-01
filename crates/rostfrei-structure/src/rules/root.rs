use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};

pub(super) struct DomainSources {
    pub(super) rust_files: Vec<PathBuf>,
    pub(super) directories: Vec<PathBuf>,
}

pub(super) fn check_required(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> bool {
    if !root.is_dir() {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            root,
            1,
            "configured domain root is not a directory",
        ));
        return false;
    }

    for required_path in [root.join("mod.rs"), root.join("model.rs")] {
        if !required_path.is_file() {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                required_path,
                1,
                "typed domain root requires this source file",
            ));
        }
    }
    let tests_module = root.join("tests").join("mod.rs");
    if !tests_module.is_file() {
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::InvalidStructure,
            tests_module,
            1,
            "typed domain root requires a sibling `tests/` module tree",
        ));
    }
    true
}

pub(super) fn collect(root: &Path, diagnostics: &mut Vec<Diagnostic>) -> DomainSources {
    let mut sources = DomainSources {
        rust_files: Vec::new(),
        directories: vec![root.to_path_buf()],
    };
    collect_directory(root, &mut sources, diagnostics);
    sources.rust_files.sort();
    sources.directories.sort();
    sources
}

fn collect_directory(
    directory: &Path,
    sources: &mut DomainSources,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                directory,
                1,
                format!("unable to read domain directory: {error}"),
            ));
            return;
        }
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.directories.push(path.clone());
            collect_directory(&path, sources, diagnostics);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.rust_files.push(path);
        }
    }
}
