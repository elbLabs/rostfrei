use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use rostfrei_structure::{
    CheckOptions, check_workspace, create_project_with_setup, scaffold_dependency,
};

mod agent_tools;

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
    /// Executes one JSON v2 behavior definition through an isolated real Tracer suite.
    Test(TestArgs),
    /// Installs and identifies release-matched portable Agent Skills.
    Skills(SkillsArgs),
}

#[derive(Args, Debug)]
struct TestArgs {
    definition: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Kit release metadata; otherwise discovered beside installed example assets.
    #[arg(long)]
    release: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Subcommand, Debug)]
enum SkillsCommand {
    /// Prints the CLI build revision, framework version and supported contracts as JSON.
    Info,
    /// Installs skills matching the application's resolved Rostfrei dependency.
    Install(InstallArgs),
    /// Updates unmodified managed skills to the application's current dependency.
    Update(InstallArgs),
    // Internal versioned-tool handoff, after the parent has checked provenance.
    #[command(hide = true)]
    Apply {
        #[arg(long)]
        kit: Option<PathBuf>,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        update: bool,
    },
}

#[derive(Args, Debug)]
struct InstallArgs {
    /// Import an explicit kit instead of resolving an application dependency.
    #[arg(long)]
    kit: Option<PathBuf>,
    /// Override the selected application's .agents/skills directory.
    #[arg(long)]
    destination: Option<PathBuf>,
    /// Application/workspace manifest (otherwise found from the current directory).
    #[arg(long, conflicts_with = "kit")]
    manifest_path: Option<PathBuf>,
    /// Select an application in a workspace with multiple default members.
    #[arg(long, conflicts_with = "kit")]
    package: Option<String>,
}

fn setup_skills(args: InstallArgs, update: bool) -> Result<bool, String> {
    agent_tools::setup_skills(agent_tools::SetupOptions {
        kit: args.kit,
        destination: args.destination,
        manifest_path: args.manifest_path,
        package: args.package,
        update,
    })
    .map_err(|error| serde_json::json!({"error": error}).to_string())?;
    Ok(true)
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
    if let Command::Test(test) = arguments.command {
        return match agent_tools::run_test(&agent_tools::TestOptions {
            definition: test.definition,
            output: test.output,
            release: test.release,
        }) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("{}", serde_json::json!({"error": error}));
                ExitCode::from(2)
            }
        };
    }
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
        Command::Skills(arguments) => match arguments.command {
            SkillsCommand::Info => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&agent_tools::Release::current())
                        .map_err(|error| error.to_string())?
                );
                Ok(true)
            }
            SkillsCommand::Install(args) => setup_skills(args, false),
            SkillsCommand::Update(args) => setup_skills(args, true),
            SkillsCommand::Apply {
                kit,
                destination,
                update,
            } => {
                agent_tools::install_skills(kit.as_deref(), &destination, update)
                    .map_err(|error| serde_json::json!({"error": error}).to_string())?;
                Ok(true)
            }
        },
        Command::Test(_) => {
            Err("Test execution must use its dedicated exit-code boundary".to_owned())
        }
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
    let (release, revision, supports_skills) = scaffold_dependency();
    let project = create_project_with_setup(arguments.path, |staged| {
        if supports_skills {
            agent_tools::setup_scaffold(staged, release.trim_start_matches('v'), revision)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    println!(
        "Created Rostfrei project `{}` at {}",
        project.package_name,
        project.destination.display()
    );
    println!("\nNext steps:");
    println!("  cd {}", project.destination.display());
    println!("  cargo check");
    println!("  cargo test");
    println!("  docker compose up -d");
    println!("  cargo run");
    if !supports_skills {
        println!(
            "\nSkills not installed: the published starter {release} ({revision}) predates skills-enabled tooling."
        );
        println!(
            "After upgrading to a published skills-enabled framework release, run cargo rostfrei skills install."
        );
    }
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
