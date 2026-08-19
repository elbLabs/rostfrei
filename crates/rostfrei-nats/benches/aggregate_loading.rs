#[path = "aggregate_loading/mod.rs"]
mod suite;

use clap::Parser as _;

fn main() -> suite::BenchResult {
    let options = suite::Options::parse();
    options.validate()?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(usize::try_from(options.workers)?)
        .enable_all()
        .build()?
        .block_on(async move {
            // Measure on a runtime worker, rather than the block_on root thread.
            tokio::spawn(suite::run(options)).await?
        })
}
