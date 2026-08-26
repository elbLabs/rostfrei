# ADR 0005: NATS event-store representation and operator policy

## Status

Accepted.

## Decision

The NATS event store writes one JetStream message per domain event. Each message
contains one bounded, checksummed event envelope plus the commit identity,
operation identity, operation fingerprint, event ordinal, and event count. A
deterministic opaque subject identifies one aggregate stream. Reads walk only
that subject using JetStream direct raw-message APIs and never depend on
consumer ACK state.

A multi-event `EventBatch` is published with the NATS ADR-50 atomic batch
protocol. Every event carries one shared `Nats-Batch-Id` and a one-based
`Nats-Batch-Sequence`. The first event carries
`Nats-Expected-Last-Subject-Sequence` with the last JetStream sequence observed
for the aggregate subject. The final domain event carries
`Nats-Batch-Commit: 1`; there is no extra stored commit-marker message. Only the
final response is a PubAck, and only that PubAck completes the append. A failed
or abandoned batch exposes none of its staged events.

This requires `async-nats` 0.50's `server_2_12` feature and NATS Server 2.12.0
or newer. Batches contain at most 1,000 events, matching the initial ADR-50
server limit. Unrelated aggregate subjects do not conflict. Stored commit IDs
and operation fingerprints provide exact retry after reconnect or restart
without relying on JetStream message-ID deduplication.

Authoritative streams use file storage, Limits retention, zero max age,
`DiscardNew`, unlimited message counts, finite byte capacity, explicit replicas,
deny delete, deny purge, and no rollup. Capacity exhaustion is distinct from
conflict and unavailability. Existing stream configuration is verified before
use, including the required `allow_atomic_publish` setting.

Event-store configuration defaults to 10 GiB of stream capacity, a 2 MiB encoded
event limit, one replica, and a five-second PubAck timeout. These bounded defaults
work with a standalone development server; production deployments normally
override the replica count to match their NATS cluster durability policy.

Stream creation and updates are operator-owned through an explicit generic
provisioning API. Service startup only connects and verifies. Product stream
names and subject prefixes are supplied by the application. Deployments may
override the default capacities, replica count, and PubAck timeout.

## Consequences

Projection consumers may process event messages in global JetStream order, but
that order is not aggregate truth. Each event has its own JetStream sequence;
all events in one commit become visible atomically. NATS KV is not used and
histories are never rewritten. Incomplete commits, inconsistent atomic-batch
headers, missing events, version gaps, duplicate identities, checksum failures,
and incompatible wire schemas fail closed.

This decision replaces the unreleased one-message-per-commit prototype format.
Streams written by that prototype must be recreated; no compatibility decoder
or migration is provided for pre-release data.
