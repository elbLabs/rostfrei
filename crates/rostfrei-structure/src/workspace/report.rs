use std::path::PathBuf;

use crate::diagnostic::Diagnostic;

#[derive(Clone, Debug, Default)]
pub struct CheckReport {
    pub packages_checked: Vec<CheckedPackage>,
    pub diagnostics: Vec<PackageDiagnostic>,
}

impl CheckReport {
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct CheckedPackage {
    pub name: String,
    pub root: PathBuf,
    pub domain_root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PackageDiagnostic {
    pub package: String,
    pub package_root: PathBuf,
    pub diagnostic: Diagnostic,
}
