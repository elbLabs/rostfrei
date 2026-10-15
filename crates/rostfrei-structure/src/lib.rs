mod compiled_check;
mod diagnostic;
mod rules;
mod scaffold;
mod source;
mod workspace;

pub use diagnostic::{Diagnostic, DiagnosticCode};
pub use rules::check_domain_root;
pub use scaffold::{
    CreatedProject, NewProjectError, create_project, create_project_with_setup, scaffold_dependency,
};
pub use workspace::{
    CheckError, CheckOptions, CheckReport, CheckedPackage, PackageDiagnostic, check_workspace,
};
