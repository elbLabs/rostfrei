# ADR 0003: Stream identity, versioning, atomicity, and idempotency

## Status

Accepted.

## Decision

A stream is identified by the pair `(aggregate_type, aggregate_id)`. Versions
are one-based event positions; version zero represents an absent stream.
`ExpectedVersion` has only `NoStream` and `Exact` variants. There is no unsafe
`Any` append.

Every append carries one non-empty event batch and is all-or-none. Assigned
versions are contiguous and preserve batch order. Conflicts leave history
unchanged. Different aggregate streams never share an expected-version gate.

An operation has a caller-supplied stable operation ID and a content fingerprint.
The executor derives the commit ID from the stream and operation ID, and event
IDs from that commit ID plus each event ordinal. A retry with the same identity
and exactly the same fingerprint and content returns an accepted
`CommandReceipt::ExactReplay` containing the original events, even after later
commits. Reusing an operation, commit, or event ID
with different content fails with an identity-conflict classification.

Exact retry is a persisted semantic, not reliance on a broker duplicate window.
An accepted eventful operation returns `CommandReceipt::Appended` when newly
committed. An accepted no-event decision returns `CommandReceipt::NoEvents`, but
has no durable retry evidence in this release. Rejections likewise are not
persisted; retrying a no-event or rejected operation reruns the decision against
the then-current aggregate state.

## Consequences

Ambiguous client failures can be retried safely. Callers must derive operation
fingerprints from a stable command representation before execution. Random IDs
and wall-clock values cannot be generated inside deterministic command logic.
