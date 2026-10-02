# ADR 0038: Attempt-scoped append sessions

## Status

Proposed. Extends [ADR 0005](0005-nats-event-store.md) and
[ADR 0037](0037-bounded-context-commands-and-unit-of-work.md).
The read trust boundary is refined by
[ADR 0039](0039-trusted-event-store-reads-and-explicit-audits.md): normal session
loads retain local history checks; deep historical receipt verification is explicit.

## Context

Command execution loads aggregate histories to make a decision. Persistence then
loads them again to validate identities and obtain native optimistic-concurrency
positions. Successful NATS publish verification also reloads historical events.
Each raw history read currently issues one sequential broker request per event.

All command writes now use atomic event transactions, even when only one
aggregate participates. Reusing a single aggregate's version alone would not
cover the current command path or preserve transaction read guards.

## Decision

`EventStore::append_session` returns a store-owned `Box<dyn AppendSession>` borrowed
from that store. A session implements `EventHistory` and provides consuming
`append` and `append_transaction` methods. It retains information for one append
attempt; it does not reserve a version or lock aggregates against other writers.

`CommandExecutor` opens a new session for every handler attempt. The unit of work
loads through that session and consumes it when persisting its transaction.
Conflicts discard the session and rerun the handler with fresh history. Accepted
no-event decisions consume the session to persist acceptance and read guards (see
[ADR 0039](0039-durable-event-free-acceptance.md)). Rejection and handler errors drop
it without persistence. Simulation continues to use the read-only history API
without opening an append session.

Existing adapters remain source-compatible through a forwarding implementation.
`Arc<Store>` forwards session creation to the underlying adapter, including when
`Store` is a trait object. Custom adapters can opt into history reuse without
exposing broker positions in domain or command-handler APIs.

The NATS implementation retains locally checked histories separately from the
raw-history cache used while validating receipts. It reuses native last-subject
sequences, but still enforces them through atomic broker expectations for every
writer and read guard. Fresh receipt lookups, operation fingerprints, derived
identities, and exact-content replay checks remain mandatory.

Successful publish verification reads only the new commit suffix, bounded by the
acknowledged sequence and expected event count. It validates the stored envelopes,
checksums, batch identity, event ordering, and continuity against the retained
prefix. Transaction verification materializes the durable receipt using those
extended histories and verifies read guards and exact global batch positions.
Later commits are outside the verification boundary.

Conflicts and unavailable publish results reconcile using fresh broker reads.
This includes duplicate-message errors: NATS can report one before reporting an
optimistic-concurrency failure when another writer has committed the same
operation. If reconciliation cannot establish a replay or identity conflict, an
uncertain publish remains unavailable rather than being reported as success.

NATS sessions capture the JetStream stream's creation timestamp and reject use
after detecting recreation. Operators must still quiesce workers before deleting
or rebuilding event-store topology: the incarnation check and broker publish
are not one atomic administrative operation.

## Consequences

Normal persistence no longer rereads already-loaded event prefixes. Broker work
after the initial load depends on newly written events and participants, not
history length. Direct `EventStore` append methods also use suffix verification;
callers that need load-and-append reuse can explicitly open a session.

The real-broker request-count regression compares histories of 1 and 100 events,
including two writers and a read guard. It also runs the shared direct-append and
transaction contracts through sessions, and checks stale-session conflicts,
exact replay, content conflicts, and stream recreation. Core coverage verifies
that retries create fresh sessions through `Arc<dyn EventStore>`.

This is not snapshotting, batched initial loading, or a global cache. Initial
rehydration and historical transaction validation remain history-dependent.
Sessions retain histories for the duration of a command attempt; suffix validation
also clones and indexes the retained prefix in memory. Large-history latency and
memory benchmarks, snapshot design, and batched reads remain follow-up work.

### Historical transaction read optimization

Within an explicit history audit, transaction receipt lookup and materialization reuse a
stream handle, and an already checked transaction-first event is reused when it
belongs to the loaded commit. Independent receipt lookups are pipelined with a
maximum of eight outstanding futures/results; materialization remains ordered
and uses the read's shared raw-history cache and cutoff. Legacy receipt lookup
precedence, fresh reconciliation, and append-session incarnation checks remain
unchanged. Reads continue to use the leader-routed raw-message API.

The [September 26 experiments](../reviews/2026-09-26-history-read-experiments.md)
record request counts, release timings, integrity verification, and a separate
Direct Get batching prototype. That prototype is not part of the authoritative
loader because replica-read consistency and explicit operator configuration need
further design and testing.

Initial raw-history discovery also uses bounded parallel leader-read windows:
up to eight windows of 128 global positions, with a 256 KiB chunk target and
oversized-record progress. One ordered fold validates commits across all window
and chunk boundaries. Indexed lookahead skips empty subject gaps, while a sparse
history falls back to a single lane to bound redundant lookups. No consumer state,
Direct Get provisioning, or replica-read assumption is introduced. See the
[second experiment round](../reviews/2026-09-26-history-read-windows.md) for
the algorithm, rejected scheduling experiment, and controlled comparisons.
