//! Release benchmark workloads and Rust-only disposable-broker orchestration.

pub mod aggregate_loading;
pub mod broker;
pub mod comparison;
pub mod process;

pub type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
