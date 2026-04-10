# ADR 0005: NATS event-store representation and operator policy

## Status

Accepted.

## Decision

The NATS event store writes one JetStream message per aggregate commit. The
message contains a bounded, checksummed commit with one or more encoded domain
events. A deterministic opaque subject identifies one aggregate stream. Reads
walk only that subject using JetStream direct raw-message APIs and never depend
on consumer ACK state.

Appends use `Nats-Expected-Last-Subject-Sequence` with the last JetStream
sequence observed for that aggregate subject. This is an atomic per-aggregate
check supported by `async-nats` 0.50 and NATS Server 2.10 or newer. Unrelated
aggregate subjects therefore do not conflict. Stored commit IDs and operation
fingerprints provide exact retry after reconnect or restart and beyond the NATS
duplicate window.

Authoritative streams use file storage, Limits retention, zero max age,
`DiscardNew`, unlimited message counts, finite byte capacity, explicit replicas,
deny delete, deny purge, and no rollup. Capacity exhaustion is distinct from
conflict and unavailability. Existing stream configuration is verified before
use.

Stream creation and updates are operator-owned through an explicit generic
provisioning API. Service startup only connects and verifies. Product stream
names and capacities are supplied by the deployment.

## Consequences

Projection consumers may process commit messages in global JetStream order, but
that order is not aggregate truth. NATS KV is not used and histories are never
rewritten. Missing commits, version gaps, duplicate versions, checksum failures,
and incompatible wire schemas fail closed.
