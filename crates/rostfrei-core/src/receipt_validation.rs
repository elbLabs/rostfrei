//! Checked receipt boundaries shared by preparation and recovered-result consumers.
use std::collections::HashSet;

use crate::{
    EventStoreError, EventStoreErrorKind, EventTransaction, ExpectedVersion, StreamVersion,
    TransactionReceipt, derive_commit_id, derive_event_id, validate_transaction_item_limit,
};

impl TransactionReceipt {
    /// Checks self-contained event integrity without requiring reclaimable details
    /// or a speculative proposal. Suitable for recovered permanent journal receipts.
    pub fn validate_integrity(&self) -> Result<(), EventStoreError> {
        let mut streams = HashSet::new();
        for stream in self.streams() {
            if !streams.insert(stream.stream_id()) {
                return Err(corrupt("duplicate receipt participant"));
            }
            let commit = derive_commit_id(stream.stream_id(), self.operation_id());
            let count = u32::try_from(stream.events().len())
                .map_err(|_| corrupt("receipt event count overflow"))?;
            let mut version = stream.base_version();
            for (ordinal, event) in stream.events().iter().enumerate() {
                version = version
                    .next()
                    .ok_or_else(|| corrupt("receipt stream version overflow"))?;
                let ordinal =
                    u32::try_from(ordinal).map_err(|_| corrupt("receipt ordinal overflow"))?;
                if event.stream_id() != stream.stream_id()
                    || event.stream_version() != version
                    || event.commit_id() != &commit
                    || event.event_id() != &derive_event_id(&commit, ordinal)
                    || event.operation_id() != self.operation_id()
                    || event.operation_fingerprint() != self.operation_fingerprint()
                    || event.correlation_id() != self.correlation_id()
                    || event.causation_id() != self.causation_id()
                    || event.commit_event_ordinal() != ordinal
                    || event.commit_event_count() != count
                {
                    return Err(corrupt(
                        "receipt event coordinates or provenance differ from its participant",
                    ));
                }
            }
        }
        if let Some(completion) = self.completion() {
            completion.validate().map_err(|e| corrupt(e.to_string()))?;
            if completion.rejection.is_some()
                && self.streams().iter().any(|s| !s.events().is_empty())
            {
                return Err(corrupt("rejected receipt contains events"));
            }
        }
        Ok(())
    }

    /// Validates this exact ordered proposal/receipt pair at preparation time.
    /// This is not proposal replay equality: expected versions are checked here,
    /// while `ProposalDigest` intentionally excludes them for exact-write retries.
    pub fn validate_proposal(&self, proposal: &EventTransaction) -> Result<(), EventStoreError> {
        if proposal.completion().is_some() {
            crate::validate_completed_transaction(proposal)?;
        }
        validate_transaction_item_limit(proposal)?;
        self.validate_integrity()
            .map_err(|e| invalid(e.to_string()))?;
        if self.bounded_context() != proposal.bounded_context()
            || self.operation_id() != proposal.operation_id()
            || self.operation_fingerprint() != proposal.operation_fingerprint()
            || self.correlation_id() != proposal.correlation_id()
            || self.causation_id() != proposal.causation_id()
            || self.security_scope() != proposal.security_scope()
            || self
                .completion()
                .is_some_and(|value| Some(value) != proposal.completion())
            || self.streams().len() != proposal.participants().len()
        {
            return Err(invalid(
                "outcome integrity receipt does not match its proposal",
            ));
        }
        for (stream, participant) in self.streams().iter().zip(proposal.participants()) {
            let base = match participant.expected_version() {
                ExpectedVersion::NoStream => StreamVersion::ZERO,
                ExpectedVersion::Exact(version) if version != StreamVersion::ZERO => version,
                ExpectedVersion::Exact(_) => {
                    return Err(invalid("Exact requires a nonzero base version"));
                }
            };
            if stream.stream_id() != participant.stream_id() || stream.base_version() != base {
                return Err(invalid(
                    "receipt participant order, identity or base version differs from proposal",
                ));
            }
            let Some(batch) = participant.batch() else {
                if !stream.events().is_empty() {
                    return Err(invalid("read guard receipt contains events"));
                }
                continue;
            };
            if batch.operation_id() != self.operation_id()
                || batch.operation_fingerprint() != self.operation_fingerprint()
                || batch.correlation_id() != self.correlation_id()
                || batch.causation_id() != self.causation_id()
                || batch.commit_id() != &derive_commit_id(stream.stream_id(), self.operation_id())
                || batch.events().len() != stream.events().len()
            {
                return Err(invalid(
                    "receipt batch metadata or cardinality differs from proposal",
                ));
            }
            for (new, recorded) in batch.events().iter().zip(stream.events()) {
                if new.event_id() != recorded.event_id()
                    || new.event_type() != recorded.event_type()
                    || new.schema_version() != recorded.schema_version()
                    || new.payload() != recorded.payload()
                {
                    return Err(invalid("receipt event content differs from proposal"));
                }
            }
        }
        Ok(())
    }
}

fn corrupt(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::CorruptHistory, message)
}
fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}
