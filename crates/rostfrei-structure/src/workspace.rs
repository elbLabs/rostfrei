mod configuration;
mod error;
mod metadata;
mod report;

use std::path::{Path, PathBuf};

use crate::compiled_check::{check_compiled_domain, check_domain_target};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::rules::check_domain_root;
use configuration::{StructureConfiguration, structure_configuration};
pub use error::CheckError;
use metadata::{Metadata, Package, load_metadata};
pub use report::{CheckReport, CheckedPackage, PackageDiagnostic};

#[derive(Clone, Debug, Default)]
pub struct CheckOptions {
    pub manifest_path: Option<PathBuf>,
}

pub fn check_workspace(options: &CheckOptions) -> Result<CheckReport, CheckError> {
    let metadata = load_metadata(options)?;
    Ok(check_metadata(metadata))
}

fn check_metadata(mut metadata: Metadata) -> CheckReport {
    metadata
        .packages
        .sort_by(|left, right| left.name.cmp(&right.name));
    let mut report = CheckReport::default();
    let mut compiled_packages = Vec::new();
    for package in metadata.packages {
        if metadata.workspace_members.contains(&package.id) {
            check_package(package, &mut report, &mut compiled_packages);
        }
    }
    for package in compiled_packages {
        if let Err(diagnostic) = check_compiled_domain(
            &package.name,
            &package.manifest_path,
            &metadata.workspace_root,
        ) {
            report.diagnostics.push(PackageDiagnostic {
                package: package.name,
                package_root: package.root,
                diagnostic,
            });
        }
    }
    report.diagnostics.sort_by(|left, right| {
        (&left.package, &left.diagnostic.path, left.diagnostic.line).cmp(&(
            &right.package,
            &right.diagnostic.path,
            right.diagnostic.line,
        ))
    });
    report
}

fn check_package(
    package: Package,
    report: &mut CheckReport,
    compiled_packages: &mut Vec<CompiledPackage>,
) {
    let Some(configuration) = structure_configuration(&package.metadata) else {
        return;
    };
    let Some(package_root) = package.manifest_path.parent().map(Path::to_path_buf) else {
        return;
    };
    match configuration {
        Ok(configuration) if configuration.version == 1 => {
            check_configured_package(
                package,
                package_root,
                configuration,
                report,
                compiled_packages,
            );
        }
        Ok(configuration) => report.diagnostics.push(PackageDiagnostic {
            package: package.name,
            package_root,
            diagnostic: Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                package.manifest_path,
                1,
                format!(
                    "unsupported Rostfrei structure version {}; expected version 1",
                    configuration.version
                ),
            ),
        }),
        Err(error) => report.diagnostics.push(PackageDiagnostic {
            package: package.name,
            package_root,
            diagnostic: Diagnostic::new(
                DiagnosticCode::InvalidStructure,
                package.manifest_path,
                1,
                format!("invalid [package.metadata.rostfrei.structure]: {error}"),
            ),
        }),
    }
}

fn check_configured_package(
    package: Package,
    package_root: PathBuf,
    configuration: StructureConfiguration,
    report: &mut CheckReport,
    compiled_packages: &mut Vec<CompiledPackage>,
) {
    let domain_root = package_root.join(configuration.domain_root);
    let diagnostics = check_domain_root(&domain_root);
    let structure_is_valid = diagnostics.is_empty();
    report
        .diagnostics
        .extend(diagnostics.into_iter().map(|diagnostic| PackageDiagnostic {
            package: package.name.clone(),
            package_root: package_root.clone(),
            diagnostic,
        }));
    report.packages_checked.push(CheckedPackage {
        name: package.name.clone(),
        root: package_root.clone(),
        domain_root,
    });
    let target_is_valid =
        match check_domain_target(&package.name, &package.targets, &package.manifest_path) {
            Ok(()) => true,
            Err(diagnostic) => {
                report.diagnostics.push(PackageDiagnostic {
                    package: package.name.clone(),
                    package_root: package_root.clone(),
                    diagnostic,
                });
                false
            }
        };
    if structure_is_valid && target_is_valid {
        compiled_packages.push(CompiledPackage {
            name: package.name,
            root: package_root,
            manifest_path: package.manifest_path,
        });
    }
}

struct CompiledPackage {
    name: String,
    root: PathBuf,
    manifest_path: PathBuf,
}
