//! Identity-bound, approval-controlled reads with mandatory durable disclosure auditing.
//!
//! HTTP and MCP share [`Insights`]. Source implementations are trusted adapters:
//! they must enforce the configured tenant partition and return stable record versions.

pub mod http;
mod model;
mod nats;
mod oidc;
mod policy;
mod service;
mod sources;

pub use model::*;
pub use nats::{NatsInsightsConfig, NatsInsightsJournal};
pub use oidc::{OidcAuthenticator, OidcConfig};
pub use service::{Insights, InsightsClock, InsightsJournal, InsightsSource};
pub use sources::{AggregateStateSource, EventHistorySource};

use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> Result<u64, InsightsError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or(InsightsError::Unavailable)
}

fn identifier() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
