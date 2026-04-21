# ADR 0005: NATS event-store representation and operator policy

## Status

Accepted; deployment naming policy partially superseded by ADR 0015.

## Decision

The NATS event store writes one JetStream message per domain event. Each message
contains one bounded, checksummed event envelope plus the commit identity,
operation identity, operation fingerprint, event ordinal, and event count. A
deterministic opaque subject identifies one aggregate stream. Reads walk only
that subject using JetStream direct raw-message APIs and never depend on
consumer ACK state.

A single-stream, multi-event `EventBatch` is published with the NATS ADR-50 atomic batch
protocol. Every event carries one shared `Nats-Batch-Id` and a one-based
`Nats-Batch-Sequence`. The first event carries
`Nats-Expected-Last-Subject-Sequence` with the last JetStream sequence observed
for the aggregate subject. The final domain event carries
`Nats-Batch-Commit: 1`; there is no extra stored commit-marker message. Only the
final response is a PubAck, and only that PubAck completes the append. A failed
or abandoned batch exposes none of its staged events.

An event transaction may atomically span multiple aggregate subjects in the
same JetStream stream. Its ADR-50 batch contains all domain events, one internal
guard message for each read-only participant, and a final internal transaction
receipt. A guard applies an expected sequence to another aggregate subject, so
read-only decisions are protected against write skew. The receipt is addressed
by primary aggregate stream and operation identity and is the final batch
message. Aggregate consumers filter out internal subjects and use transaction
event ordinals to deliver only complete domain-event groups.

This requires `async-nats` 0.50's `server_2_12` feature and NATS Server 2.12.1
or newer. An ADR-50 batch contains at most 1,000 items. A direct single-stream
append can contain 1,000 events; an event transaction budgets its domain events,
read guards, and receipt against the same limit. Unrelated aggregate subjects do
not conflict. Stored commit IDs, operation fingerprints, and transaction
receipts provide exact retry after reconnect or restart without relying on
JetStream message-ID deduplication.

Authoritative streams use file storage, Limits retention, zero max age,
`DiscardNew`, unlimited message counts, finite byte capacity, explicit replicas,
deny delete, deny purge, and no rollup. Capacity exhaustion is distinct from
conflict and unavailability. Existing stream configuration is verified before
use, including the required `allow_atomic_publish` setting.

Event-store configuration defaults to 10 GiB of stream capacity, a 512 KiB
encoded-event write limit, one replica, and a five-second PubAck timeout. The
stream reserves a bounded 4 KiB allowance above the event limit for ADR-50
headers. Provisioning and connection reject a server whose negotiated
`max_payload` is smaller than that wire-message limit. Schemas 1 through 3 retain
the previous 2 MiB read limit, and provisioning preserves a larger existing
stream message limit, so lowering the default does not make existing history
unreadable.

Stream creation and updates are operator-owned through an explicit provisioning
API. Service startup only connects and verifies. The normal stream name and
subject prefix are derived from a bounded context. The exceptional constructor
permits a custom stream name while retaining application and bounded-context
subject scope. Deployments may override the default capacities, replica count,
and PubAck timeout through builders.
Provisioning upgrades an event-store stream with the previous aggregate-only
subject list by adding the internal transaction subject. Other subject-list
mismatches still fail as scope conflicts. Provisioning checks the NATS version
before creating or updating the stream.

Wire schema 4 and the internal transaction subjects are not understood by older
rostfrei binaries. Deployments must therefore stop old event-store readers and
writers, deploy transaction-aware binaries, provision the stream, and only then
enable transaction writes. Mixed-version rolling operation is unsupported for
this schema upgrade.

## Consequences

Projection consumers may process event messages in global JetStream order, but
that order is not aggregate truth. Each event has its own JetStream sequence;
all events in one commit become visible atomically. NATS KV is not used and
histories are never rewritten. Incomplete commits, inconsistent atomic-batch
headers, missing events, version gaps, duplicate identities, checksum failures,
and incompatible wire schemas fail closed.

Atomic event transactions cannot cross JetStream streams. Aggregates that must
commit together therefore belong to one bounded-context event-store stream;
cross-stream or cross-service coordination uses explicit process managers or
sagas instead.

This decision replaces the unreleased one-message-per-commit prototype format.
Streams written by that prototype must be recreated; no compatibility decoder
or migration is provided for pre-release data.
