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

See [runtime operations](../runtime.md), [native authority](../command-runtime.md#native-authority),
and the [cutover runbook](../completed-outcome-cutover.md).
