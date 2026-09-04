use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use rostfrei_structure::{CheckOptions, check_workspace, create_project};

#[derive(Debug, Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum CargoCli {
    #[command(name = "rostfrei")]
    Rostfrei(RostfreiArgs),
}

#[derive(Args, Debug)]
#[command(author, version, about = "Rostfrei project tools")]
struct RostfreiArgs {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks that configured packages follow the Rostfrei domain structure.
    Check(CheckArgs),
    /// Creates a new Rostfrei project.
    New(NewArgs),
}

#[derive(Args, Debug)]
struct CheckArgs {
    /// Checks all configured workspace packages.
    #[arg(long = "workspace")]
    workspace: bool,
    /// Path to the Cargo manifest to check.
    #[arg(long)]
    manifest_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct NewArgs {
    /// Directory to create. Its name is used as the package and context name.
    path: PathBuf,
}

fn main() -> ExitCode {
    let CargoCli::Rostfrei(arguments) = CargoCli::parse();
    match run(arguments.command) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<bool, String> {
    match command {
        Command::Check(arguments) => run_check(arguments),
        Command::New(arguments) => run_new(arguments),
    }
}

fn run_check(arguments: CheckArgs) -> Result<bool, String> {
    let CheckArgs {
        workspace: _,
        manifest_path,
    } = arguments;
    let options = CheckOptions { manifest_path };
    let report = check_workspace(&options).map_err(|error| error.to_string())?;
    for package_diagnostic in &report.diagnostics {
        eprintln!(
            "package `{}`\n{}\n",
            package_diagnostic.package,
            package_diagnostic
                .diagnostic
                .render(&package_diagnostic.package_root)
        );
    }
    if report.is_success() {
        println!(
            "rostfrei check passed ({} configured package(s))",
            report.packages_checked.len()
        );
    } else {
        eprintln!(
            "rostfrei check failed with {} diagnostic(s)",
            report.diagnostics.len()
        );
    }
    Ok(report.is_success())
}

fn run_new(arguments: NewArgs) -> Result<bool, String> {
    let project = create_project(arguments.path).map_err(|error| error.to_string())?;
    println!(
        "Created Rostfrei project `{}` at {}",
        project.package_name,
        project.destination.display()
    );
    println!("\nNext steps:");
    println!("  cd {}", project.destination.display());
    println!("  docker compose up -d");
    println!("  cargo run");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{CargoCli, Command};

    #[test]
    fn clap_definition_is_valid() {
        CargoCli::command().debug_assert();
    }

    #[test]
    fn parses_the_cargo_check_subcommand() {
        let parsed = CargoCli::try_parse_from([
            "cargo",
            "rostfrei",
            "check",
            "--workspace",
            "--manifest-path",
            "project/Cargo.toml",
        ]);
        assert!(parsed.is_ok(), "check command did not parse: {parsed:?}");
        let Ok(CargoCli::Rostfrei(arguments)) = parsed else {
            return;
        };
        let Command::Check(check) = arguments.command else {
            panic!("expected check command");
        };
        assert!(check.workspace);
        assert_eq!(
            check.manifest_path.as_deref(),
            Some(std::path::Path::new("project/Cargo.toml"))
        );
    }

    #[test]
    fn parses_the_cargo_new_subcommand() {
        let parsed = CargoCli::try_parse_from(["cargo", "rostfrei", "new", "bike-rental"]);
        assert!(parsed.is_ok(), "new command did not parse: {parsed:?}");
        let Ok(CargoCli::Rostfrei(arguments)) = parsed else {
            return;
        };
        let Command::New(new) = arguments.command else {
            panic!("expected new command");
        };
        assert_eq!(new.path, std::path::Path::new("bike-rental"));
    }
}
