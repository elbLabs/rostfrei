# NATS persistence

**Preserve authoritative facts; make acceleration replaceable.**

Stored events and completed decisions are the durable evidence of what happened.
Faster reconstruction and shorter-lived response bodies must not change that
evidence or weaken its integrity.

## What follows

- Keep the NATS journal append-only, checksummed, and without age expiry. Finite
  capacity refuses new writes rather than evicting authoritative history.
- Publish events, read guards, and permanent outcome evidence in one atomic batch.
  Prepared result details alone do not establish completion; only the journal
  selects the winner. Explicit detail expiry leaves terminal decision evidence.
- Bind coordinated authority to its epoch and resource incarnations. Replacing
  storage must not silently admit stale writers or reuse evidence from a prior
  authority. Backup and restore preserve the journal/outcome-state pair.
- Prefer native NATS features and official client APIs for batching, ordering,
  retries, and recovery. Keep domain validation and consistency checks; custom
  transport machinery needs a concrete gap that the native facilities cannot fill.
- Read at a captured cutoff without advancing application durable consumers.
  Validate identities, continuity, and commit structure; keep deeper historical
  audits explicit. An optimization must preserve the same committed result.
- Derived state never justifies deleting original events or returning partial
  state as complete. Losing an optimization must be distinguishable from losing
  authoritative history.
- Provision, migrate, and change retention policy explicitly. Startup verifies
  resources rather than silently recreating or weakening them.

## Tradeoff

Permanent history needs capacity planning. Trusted reads avoid repeated deep
audits but assume supported writes; explicit audits check historical transaction
consistency. Atomicity stops at one journal, and authority lifecycle management
adds coordination work.

### Subject-addressed inspection

Putting correlation and causation in [event subjects](../messaging/subjects.mdx#stored-event-subjects)
allows native retained correlation reads without a separate index/projector or its
freshness lag. It trades that application infrastructure for more broker indexing
and aggregate-write work.

Domain-event subjects identify distinct `(type, id, correlation, causation)`
combinations. Events within one commit normally share a subject; a new cause or
correlation creates another. Their count can therefore approach the number of
aggregate commits rather than aggregates. These entries never age out of the
journal. JetStream's per-subject bookkeeping adds memory and index restore/rebuild
work as cardinality grows, including for file-backed streams. Permanent transaction
and read-guard subjects already contribute separate operation-scoped growth.

Aggregate-wide optimistic concurrency uses
`Nats-Expected-Last-Subject-Sequence-Subject` with `<type>.<id>.*.*`. In NATS 2.12.15,
the file store's wildcard last-message lookup traverses matching subject-index
entries, unlike its exact-subject lookup. This adds work proportional to the
aggregate's distinct subjects on each concurrency check; it need not reread every
event payload. Busy, long-lived aggregates therefore have a growing write-path
cost. Capacity qualification must cover subject cardinality, restart recovery,
and writes to aged aggregates, not only event bytes or fresh-namespace throughput.

Causation remains authoritative in event envelopes. Its subject token currently
has no non-test query caller in this repository; retained inspection selects by
correlation and reads causation from the envelope. Removing that token would
reduce domain-event subjects to distinct `(type, id, correlation)` combinations,
helping when a correlation revisits an aggregate. It would still create a subject
for each new aggregate/correlation pair and require wildcard concurrency checks.
Integration-event subject growth is limited by configured retention (30 days by
default); domain journals have no equivalent expiry.

See [runtime operations](../runtime.md), [native authority](../command-runtime.md#native-authority),
and the [cutover runbook](../completed-outcome-cutover.md).
