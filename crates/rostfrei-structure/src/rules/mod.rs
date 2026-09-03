mod action_ownership;
mod content;
mod evaluation_ownership;
mod files;
mod hierarchy;
mod modules;
mod query_ownership;
mod roles;
mod root;
mod test_mirror;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::source::{self, SourceFileFacts};

pub fn check_domain_root(root: &Path) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if !root::check_required(root, &mut diagnostics) {
        return diagnostics;
    }

    let sources = root::collect(root, &mut diagnostics);
    let facts = parse_sources(&sources.rust_files, &mut diagnostics);

    files::check(root, &facts, &mut diagnostics);
    action_ownership::check(root, &sources.directories, &facts, &mut diagnostics);
    evaluation_ownership::check(root, &sources.directories, &facts, &mut diagnostics);
    query_ownership::check(root, &sources.directories, &facts, &mut diagnostics);
    content::check(root, &facts, &mut diagnostics);
    modules::check(root, &facts, &mut diagnostics);
    hierarchy::check(root, &sources.directories, &facts, &mut diagnostics);
    test_mirror::check(
        root,
        &sources.directories,
        &sources.rust_files,
        &mut diagnostics,
    );
    sort_diagnostics(&mut diagnostics);
    diagnostics
}

fn parse_sources(
    paths: &[PathBuf],
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<PathBuf, SourceFileFacts> {
    let mut facts = BTreeMap::new();
    for path in paths {
        match source::parse(path) {
            Ok(file_facts) => {
                facts.insert(path.clone(), file_facts);
            }
            Err(error) => diagnostics.push(Diagnostic::new(
                DiagnosticCode::SourceParse,
                path,
                error.span().start().line,
                format!("unable to parse Rust source: {error}"),
            )),
        }
    }
    facts
}

fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        (&left.path, left.line, left.code, &left.message).cmp(&(
            &right.path,
            right.line,
            right.code,
            &right.message,
        ))
    });
}
