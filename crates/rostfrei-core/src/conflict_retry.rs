use std::time::Duration;

use crate::EventStoreError;

/// Supplied only after a genuine optimistic conflict and unsuccessful winner reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictRetry {
    evaluations: usize,
    error: EventStoreError,
}

impl ConflictRetry {
    pub(crate) const fn new(evaluations: usize, error: EventStoreError) -> Self {
        Self { evaluations, error }
    }
    pub const fn evaluations(&self) -> usize {
        self.evaluations
    }
    pub const fn error(&self) -> &EventStoreError {
        &self.error
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictRetryDecision {
    Stop,
    RetryNow,
    RetryAfter(Duration),
}

pub trait ConflictRetryPolicy: Send + Sync {
    fn after_conflict(&self, conflict: ConflictRetry) -> ConflictRetryDecision;
}

#[derive(Clone, Copy, Debug)]
pub struct BoundedConflictRetries {
    maximum: usize,
}

impl BoundedConflictRetries {
    pub const fn new(maximum: usize) -> Self {
        Self { maximum }
    }
}

impl Default for BoundedConflictRetries {
    fn default() -> Self {
        Self::new(3)
    }
}

impl ConflictRetryPolicy for BoundedConflictRetries {
    fn after_conflict(&self, conflict: ConflictRetry) -> ConflictRetryDecision {
        if conflict.evaluations() <= self.maximum {
            ConflictRetryDecision::RetryNow
        } else {
            ConflictRetryDecision::Stop
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoConflictRetries;
impl ConflictRetryPolicy for NoConflictRetries {
    fn after_conflict(&self, _: ConflictRetry) -> ConflictRetryDecision {
        ConflictRetryDecision::Stop
    }
}
