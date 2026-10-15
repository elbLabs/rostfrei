use sha2::{Digest, Sha256};

use crate::identity::hash_part;
use crate::{ContentFingerprint, EventStoreError, EventStoreErrorKind, EventTransaction};

/// Version 1: SHA-256 over framed fields, ordered participants/batches and completion JSON v1.
///
/// Expected versions are deliberately excluded (the
/// original append contract), while timestamp and rejection bytes are included.
/// This is proposal equality, NOT command identity or winner reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProposalDigest {
    version: u16,
    digest: ContentFingerprint,
}

impl ProposalDigest {
    pub(crate) fn restore(
        version: u16,
        digest: ContentFingerprint,
    ) -> Result<Self, EventStoreError> {
        if version != 1 {
            return Err(EventStoreError::new(
                EventStoreErrorKind::CorruptHistory,
                "unsupported proposal digest version",
            ));
        }
        Ok(Self { version, digest })
    }
    pub fn of(transaction: &EventTransaction) -> Result<Self, EventStoreError> {
        let mut hash = Sha256::new();
        hash_part(&mut hash, b"rostfrei:proposal:v1");
        optional(
            &mut hash,
            transaction
                .bounded_context()
                .map(rostfrei_messaging_core::BoundedContextName::as_str),
        );
        hash_part(&mut hash, transaction.operation_id().as_str().as_bytes());
        hash_part(&mut hash, transaction.operation_fingerprint().as_bytes());
        optional(
            &mut hash,
            transaction
                .correlation_id()
                .map(rostfrei_messaging_core::CorrelationId::as_str),
        );
        optional(
            &mut hash,
            transaction
                .causation_id()
                .map(rostfrei_messaging_core::CausationId::as_str),
        );
        optional(
            &mut hash,
            transaction
                .security_scope()
                .map(crate::SecurityScope::as_str),
        );
        let completion = serde_json::to_vec(&transaction.completion()).map_err(|error| {
            EventStoreError::new(EventStoreErrorKind::InvalidRequest, error.to_string())
        })?;
        hash_part(&mut hash, &completion);
        for participant in transaction.participants() {
            hash_part(&mut hash, b"participant");
            hash_part(
                &mut hash,
                participant.stream_id().aggregate_type().as_str().as_bytes(),
            );
            hash_part(
                &mut hash,
                participant.stream_id().aggregate_id().as_str().as_bytes(),
            );
            if let Some(batch) = participant.batch() {
                hash_part(&mut hash, b"batch");
                hash_part(&mut hash, batch.commit_id().as_str().as_bytes());
                hash_part(&mut hash, batch.operation_id().as_str().as_bytes());
                hash_part(&mut hash, batch.operation_fingerprint().as_bytes());
                optional(
                    &mut hash,
                    batch
                        .correlation_id()
                        .map(rostfrei_messaging_core::CorrelationId::as_str),
                );
                optional(
                    &mut hash,
                    batch
                        .causation_id()
                        .map(rostfrei_messaging_core::CausationId::as_str),
                );
                for event in batch.events() {
                    hash_part(&mut hash, b"event");
                    hash_part(&mut hash, event.event_id().as_str().as_bytes());
                    hash_part(&mut hash, event.event_type().as_bytes());
                    hash_part(&mut hash, &event.schema_version().to_be_bytes());
                    hash_part(&mut hash, event.payload());
                }
                hash_part(&mut hash, b"end-batch");
            } else {
                hash_part(&mut hash, b"guard");
            }
        }
        hash_part(&mut hash, b"end-proposal");
        Ok(Self {
            version: 1,
            digest: ContentFingerprint::from_bytes(hash.finalize().into()),
        })
    }

    pub const fn version(self) -> u16 {
        self.version
    }
    pub const fn fingerprint(self) -> ContentFingerprint {
        self.digest
    }
}

fn optional(hash: &mut Sha256, value: Option<&str>) {
    hash_part(hash, if value.is_some() { b"some" } else { b"none" });
    if let Some(value) = value {
        hash_part(hash, value.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AggregateId, AggregateType, ExpectedVersion, OperationId, StreamId, StreamVersion,
        TransactionParticipant,
    };

    #[test]
    fn version_one_framing_vector_and_expected_version_compatibility() {
        let transaction = EventTransaction::new(
            OperationId::new("vector").unwrap(),
            ContentFingerprint::digest("command"),
            Vec::new(),
        );
        let digest = ProposalDigest::of(&transaction).unwrap();
        assert_eq!(digest.version(), 1);
        // Independently calculated with Python hashlib and eight-byte big-endian framing.
        assert_eq!(
            digest.fingerprint().to_hex(),
            "3bd8f1acdbbd5888ad7da3e615ae446663df585670594e0266f78241e4b2355b"
        );
        let guarded = |version| {
            EventTransaction::new(
                transaction.operation_id().clone(),
                transaction.operation_fingerprint(),
                vec![TransactionParticipant::new(
                    StreamId::new(
                        AggregateType::new("counter").unwrap(),
                        AggregateId::new("a").unwrap(),
                    ),
                    version,
                    None,
                )],
            )
        };
        let absent = ProposalDigest::of(&guarded(ExpectedVersion::NoStream)).unwrap();
        assert_eq!(
            absent,
            ProposalDigest::of(&guarded(ExpectedVersion::Exact(StreamVersion::new(42)))).unwrap()
        );
        assert_ne!(digest, absent);
        assert_ne!(
            digest,
            ProposalDigest::of(
                &transaction.with_security_scope(crate::SecurityScope::new("scope").unwrap())
            )
            .unwrap()
        );
    }
}
