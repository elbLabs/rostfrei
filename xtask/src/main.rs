use anyhow::Result;
use clap::{Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode, time::Duration};
use xtask::{acceptance, candidate, live, nats, process, root, skills, versions};

#[derive(Parser)]
#[command(about = "Rostfrei repository development and release tooling")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Execute a command against an owned disposable NATS broker.
    Test {
        #[arg(long)]
        contracts: bool,
        #[arg(long, default_value = "30", value_parser = positive)]
        ready_timeout: f64,
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Execute against an authenticated native broker and retain diagnostic evidence.
    Contracts {
        #[arg(long)]
        nats_binary: PathBuf,
        #[arg(long)]
        evidence_dir: PathBuf,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    #[command(subcommand)]
    Versions(Version),
    #[command(subcommand)]
    Skills(Skill),
    #[command(subcommand)]
    Acceptance(Acceptance),
}
#[derive(Subcommand)]
enum Version {
    Check {
        #[arg(long)]
        tag: Option<String>,
    },
    Bump {
        version: String,
    },
}
#[derive(Subcommand)]
enum Skill {
    Package {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        require_clean: bool,
    },
    Validate {
        #[arg(required = true)]
        directories: Vec<PathBuf>,
    },
    Candidate {
        #[arg(
            long,
            required_unless_present = "prepare_app",
            conflicts_with = "prepare_app"
        )]
        output: Option<PathBuf>,
        #[arg(long)]
        prepare_app: Option<PathBuf>,
    },
    Live(live::Options),
}
#[derive(Subcommand)]
enum Acceptance {
    NewProject,
    TracerHost {
        #[arg(long)]
        working_tree: bool,
    },
    BikeRentalHost {
        #[arg(long)]
        studio: bool,
    },
    Skills,
    Onboarding,
    Source,
}
fn positive(text: &str) -> Result<f64, String> {
    let value: f64 = text.parse().map_err(|_| "expected seconds".to_owned())?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err("timeout must be positive and finite".into())
    }
}
fn run(cli: Cli) -> Result<i32> {
    match cli.command {
        Task::Test {
            contracts,
            ready_timeout,
            command,
        } => return nats::run(command, contracts, Duration::from_secs_f64(ready_timeout)),
        Task::Contracts {
            nats_binary,
            evidence_dir,
            command,
        } => return nats::native(nats_binary, evidence_dir, command),
        Task::Versions(Version::Check { tag }) => println!(
            "Shared crate versions are consistent: {}",
            versions::check(&root(), tag.as_deref())?
        ),
        Task::Versions(Version::Bump { version }) => {
            versions::bump(&root(), &version)?;
            println!("Updated shared version to {version}");
        }
        Task::Skills(Skill::Package {
            output,
            require_clean,
        }) => {
            println!(
                "{}",
                skills::package(&xtask::absolute(&output)?, require_clean)?.display()
            );
        }
        Task::Skills(Skill::Validate { directories }) => {
            for directory in directories {
                skills::validate(&directory)?;
                println!("{}: valid", directory.display());
            }
        }
        Task::Skills(Skill::Candidate {
            output,
            prepare_app,
        }) => {
            if let Some(app) = prepare_app {
                candidate::prepare_bike_rental(
                    &root(),
                    &xtask::absolute(&app)?,
                    &candidate::clean_revision(&root())?,
                )?;
            } else {
                candidate::qualify(&xtask::absolute(&output.expect("clap output"))?)?;
            }
        }
        Task::Skills(Skill::Live(options)) => live::qualify(options)?,
        Task::Acceptance(Acceptance::NewProject) => acceptance::new_project()?,
        Task::Acceptance(Acceptance::TracerHost { working_tree }) => {
            acceptance::tracer_host(working_tree)?
        }
        Task::Acceptance(Acceptance::BikeRentalHost { studio }) => acceptance::bike_host(studio)?,
        Task::Acceptance(Acceptance::Skills) => acceptance::skills()?,
        Task::Acceptance(Acceptance::Onboarding) => acceptance::onboarding()?,
        Task::Acceptance(Acceptance::Source) => acceptance::source()?,
    }
    Ok(0)
}
fn main() -> ExitCode {
    let result = process::install_signals().and_then(|()| run(Cli::parse()));
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}");
            process::signal_code().unwrap_or(1)
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_host_command_preserves_its_flags() {
        let cli = Cli::try_parse_from([
            "xtask",
            "skills",
            "live",
            "--output",
            "evidence",
            "--host-build-provenance",
            "build.json",
            "--host-command",
            "bike-rental-api",
            "--tracer",
        ])
        .unwrap();
        let Task::Skills(Skill::Live(options)) = cli.command else {
            panic!("live command");
        };
        assert_eq!(options.host_command, ["bike-rental-api", "--tracer"]);
    }
}
