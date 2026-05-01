use std::fmt;
use std::path::Path;
use std::process::{Command, Output};

use super::target::DOMAIN_CHECK_TARGET;

#[derive(Debug)]
pub(super) struct DomainCheckFailure {
    pub(super) message: String,
    pub(super) output: Option<String>,
}

impl fmt::Display for DomainCheckFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(super) fn run_domain_check(
    package: &str,
    package_manifest: &Path,
    workspace_root: &Path,
) -> Result<(), DomainCheckFailure> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(workspace_root)
        .args(["run", "--quiet", "--manifest-path"])
        .arg(package_manifest)
        .args(["--bin", DOMAIN_CHECK_TARGET])
        .output()
        .map_err(|error| DomainCheckFailure {
            message: format!(
                "could not execute `{DOMAIN_CHECK_TARGET}` for package `{package}`: {error}"
            ),
            output: None,
        })?;

    if output.status.success() {
        return Ok(());
    }

    let status = output.status.code().map_or_else(
        || "terminated by signal".to_owned(),
        |code| code.to_string(),
    );
    Err(DomainCheckFailure {
        message: format!(
            "compiled domain check failed for package `{package}` (exit status {status})"
        ),
        output: captured_output(&output),
    })
}

fn captured_output(output: &Output) -> Option<String> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = stderr.trim();
    let stdout = stdout.trim();

    match (stderr.is_empty(), stdout.is_empty()) {
        (true, true) => None,
        (false, true) => Some(format!("stderr:\n{stderr}")),
        (true, false) => Some(format!("stdout:\n{stdout}")),
        (false, false) => Some(format!("stderr:\n{stderr}\nstdout:\n{stdout}")),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::captured_output;

    #[test]
    fn failure_output_is_ordered_by_channel() {
        let output = Command::new("sh")
            .args(["-c", "printf output; printf error >&2; exit 1"])
            .output()
            .expect("test shell should execute");

        assert_eq!(
            captured_output(&output).as_deref(),
            Some("stderr:\nerror\nstdout:\noutput")
        );
    }
}
