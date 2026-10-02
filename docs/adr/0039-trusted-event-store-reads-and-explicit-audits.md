# ADR 0039: Trusted event-store reads and explicit historical audits

## Status

Implemented as a reversible experiment. Refines ADR 0005 and ADR 0038's
historical transaction-validation policy; event and receipt wire formats do not change.

## Context

Rostfrei already validates append requests, enforces optimistic writer/read-guard
expectations, publishes atomic batches, preserves context-scoped operation
identity, and verifies newly published data. Ordinary loads additionally reread
every historical transaction receipt and its related participant/guard evidence.

The aggregate benchmark measured 3,017 requests and about 1.70 seconds to
rehydrate a 1,000-command history. Only about 3.57 ms occurred after history
retrieval. Re-auditing old committed transactions on every load dominates that
workload and exceeds the responsibility needed for ordinary state reconstruction.

## Decision

Treat the correctly provisioned event store and its supported append API as the
authority for committed history. Separate ordinary reads from deep auditing:

- `NatsEventStore::load`, append-session loads, and `StreamDirectory::list_streams`
  decode stored records and check local history structure. They do not traverse
  historical transaction receipts, other aggregate histories, or read guards.
- `NatsEventStore::audit_history` explicitly loads and verifies a history's
  transaction receipts, participant provenance, and guards, returning the audited
  events. `audit_streams` performs the corresponding directory audit with one
  captured cutoff shared by discovery and referenced participants.
- Audits retain the existing legacy-first receipt selection and error behavior.
  They do not recursively audit unrelated transactions in referenced histories.

Ordinary reads retain envelope/schema/checksum validation, aggregate identity,
derived event/commit identities, version continuity, duplicate detection,
complete local commits, predecessor expectations, and locally provable atomic
coordinates. Native positions within each local commit must be contiguous;
transaction batch-start calculations must remain representable and nonzero.
Typed event decoding and aggregate application remain unchanged.

Incoming write validation, writer/read-guard expectations, atomic publication,
PubAck and new-suffix verification, exact replay, and uncertain-write
reconciliation remain enforced. Those paths still read the **specific operation's
receipt** when needed. They do not audit every older transaction merely because
a new command loads or writes an aggregate. Durable domain-event delivery retains
its operation-specific transaction checks.

## Trust boundary and compatibility

This deliberately changes the detection boundary for out-of-band modifications.
A structurally valid transactional event planted directly in NATS with a missing
or incompatible receipt can be returned by ordinary reads; explicit auditing
rejects it. An unrelated participant's malformed history or a receipt shadow is
not discovered by every aggregate load. Such data is outside the supported
event-store write contract; imports and administrative recovery can invoke the
audit APIs before exposing the data to application execution.

Basic corrupt local history still fails ordinary reads and append-session use.
Failed local validation never installs a usable session history. Sessions retain
incarnation checks, and operators still quiesce workers for topology rebuilds.

This is not a switch to replica reads, a new cache, a snapshot strategy, or a
relaxation of atomic writes. The bounded leader-read transport, durable receipts,
stored metadata, and operator provisioning policy remain in place.

## Verification

Tests distinguish trusted loading from auditing using an out-of-band event with
missing receipt evidence. Existing malformed-history/write tests now use broken
local predecessor expectations, retaining their no-publication and failed-cache
assertions. Legacy receipt/guard and directory-cutoff tests exercise explicit
audit paths. Request budgets require ordinary reads to scale with aggregate
events rather than historical receipt count.

The benchmark supports `--audit-reads` to exercise the previous deep-read policy
for its read phases. Writes remain ordinary. A separate release A/B test compares
trusted and audited full rehydration over the **same 1,000-command fixture** and
checks equal resulting state. Performance results are recorded alongside this
experiment; no wall-clock threshold is enforced in ordinary CI.
The [measured comparison](../benchmarks/trusted-reads-experiment.md) records
full aggregate readiness, request counts, correctness checks, and limitations.

## Rollback

The experiment is kept in a separate commit after the optimization/benchmark
baseline. Reverting that commit restores historical receipt verification during
ordinary reads. No stored data migration or rollback is required.
