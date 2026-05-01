mod command;
mod target;

use std::path::Path;

use command::run_domain_check;
pub use target::CargoTarget;
use target::{DOMAIN_CHECK_TARGET, has_domain_check_target};

use crate::diagnostic::{Diagnostic, DiagnosticCode};

pub fn check_domain_target(
    package: &str,
    targets: &[CargoTarget],
    package_manifest: &Path,
) -> Result<(), Diagnostic> {
    if has_domain_check_target(targets) {
        Ok(())
    } else {
        Err(Diagnostic::new(
            DiagnosticCode::MissingDomainCheckTarget,
            package_manifest,
            1,
            format!(
                "configured Rostfrei package `{package}` has no `{DOMAIN_CHECK_TARGET}` binary target"
            ),
        )
        .with_help(format!(
            "add `src/bin/{DOMAIN_CHECK_TARGET}.rs` to the package"
        )))
    }
}

pub fn check_compiled_domain(
    package: &str,
    package_manifest: &Path,
    workspace_root: &Path,
) -> Result<(), Diagnostic> {
    run_domain_check(package, package_manifest, workspace_root).map_err(|failure| {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::CompiledDomainCheckFailed,
            package_manifest,
            1,
            failure.message,
        );
        match failure.output {
            Some(output) => diagnostic.with_help(output),
            None => diagnostic,
        }
    })
}
