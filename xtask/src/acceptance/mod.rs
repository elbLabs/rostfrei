mod hosts;
mod installed;
mod onboarding;
mod source;
pub use hosts::{bike_host, new_project, tracer_host};
pub use installed::run as skills;
pub use onboarding::run as onboarding;
pub use source::run as source;

use crate::{Environment, process};
use anyhow::Result;
use std::{path::Path, time::Duration};

fn command(args: &[&str], cwd: &Path, env: &Environment) -> Result<process::Output> {
    process::capture(
        &process::args(args),
        cwd,
        env,
        Duration::from_secs(1800),
        &[],
        false,
    )
}
