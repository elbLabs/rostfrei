# ADR 0003: Stream identity, versioning, atomicity, and idempotency

## Status

Accepted.

## Decision

A stream is identified by the pair `(aggregate_type, aggregate_id)`. Versions
are one-based event positions; version zero represents an absent stream.
`ExpectedVersion` has only `NoStream` and `Exact` variants. There is no unsafe
`Any` append.

Every direct stream append carries one non-empty event batch and is all-or-none.
Assigned versions are contiguous and preserve batch order. Conflicts leave
history unchanged. A command transaction may atomically guard and append several
streams; each participant retains its own expected-version gate.

An operation has a caller-supplied stable operation ID and a content fingerprint.
For command transactions, the operation ID is unique within one bounded-context
`EventStore`; transaction receipts are therefore looked up without selecting a
primary stream. The executor derives a deterministic commit ID for each writing
stream from that stream and operation ID, and event IDs from the commit ID plus
each event ordinal. A retry with the same identity and exactly the same
fingerprint and content returns an accepted `CommandReceipt::ExactReplay`
containing the original events, even after later commits. Reusing a transaction
operation, commit, or event ID with different content fails with an
identity-conflict classification.

Exact retry is a persisted semantic, not reliance on a broker duplicate window.
An accepted eventful operation returns `CommandReceipt::Appended` when newly
committed. An accepted no-event decision returns `CommandReceipt::NoEvents`, but
has no durable retry evidence in this release. Rejections likewise are not
persisted; retrying a no-event or rejected operation reruns the decision against
the then-current aggregate state.

The low-level `EventStore::append` API retains stream-scoped operation identity
for fixtures, imports, and infrastructure operations. Direct append IDs and
command transaction IDs are separate identity namespaces; the store does not
provide cross-API global arbitration between them. See
[ADR 0037](0037-bounded-context-commands-and-unit-of-work.md).

## Consequences

Ambiguous client failures can be retried safely. Callers must derive operation
fingerprints from a stable command representation before execution. Random IDs
and wall-clock values cannot be generated inside deterministic command logic.
