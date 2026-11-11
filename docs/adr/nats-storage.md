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
- Bind coordinated authority to its resource incarnations. Replacing
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

Ordinary execution takes priority over occasional inspection. Domain events use
one stable subject per aggregate, preserving exact-subject history and optimistic
concurrency. Existing permanent transaction receipts carry correlation and causation
in their [addresses](../messaging/subjects.mdx#filters-and-internal-storage-subjects).
Inspection discovers those receipts and fetches their bounded event ranges, without
a projector, freshness lag, or another per-command reference record. Test-only
fixture batches have separate atomic manifests; they do not establish command decisions.

This reuses authoritative evidence but does not create an independent correlation
index: a correlation filter traverses operation branches. Receipt discovery can
therefore become expensive as the journal grows, even for a small result. Client
limits bound fetched work, not all broker index traversal or its contention with
writes. Operation lookup and winner CAS stay narrowly scoped to one operation.

Permanent receipts, guards, and fixture manifests still grow with retained writes.
JetStream's per-subject bookkeeping adds memory and index restore/rebuild work,
including with file storage. Capacity qualification must cover lifetime subject
cardinality, restart recovery, and inspection concurrent with ordinary commands.
Integration-event subjects retain both trace identities and remain independently
bounded by their retention policy (30 days by default); domain journals never age out.

See [runtime operations](../runtime.md), [native authority](../command-runtime.md#native-authority),
and the [cutover runbook](../completed-outcome-cutover.md).
