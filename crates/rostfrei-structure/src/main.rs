use std::path::PathBuf;
use std::process::ExitCode;

use rostfrei_structure::{CheckOptions, check_workspace};

const USAGE: &str = "Usage: cargo rostfrei check [--workspace] [--manifest-path <PATH>]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(success) => {
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<bool, String> {
    let mut arguments = arguments.into_iter().peekable();
    if arguments
        .peek()
        .is_some_and(|argument| argument == "rostfrei")
    {
        arguments.next();
    }
    let Some(command) = arguments.next() else {
        return Err("missing command".to_owned());
    };
    if command == "--help" || command == "-h" {
        println!("{USAGE}");
        return Ok(true);
    }
    if command != "check" {
        return Err(format!("unknown command `{command}`"));
    }

    let mut options = CheckOptions::default();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--workspace" => {}
            "--manifest-path" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| "--manifest-path requires a path".to_owned())?;
                options.manifest_path = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(true);
            }
            _ => return Err(format!("unknown argument `{argument}`")),
        }
    }

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
