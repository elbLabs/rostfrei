# ADR 0042: Batched aggregate history replay

## Status

Accepted. Extends ADRs 0005, 0038 and 0040. Implements issue #97.

## Decision

Initial aggregate-history reads capture the last message sequence on the exact
opaque aggregate subject, then replay through a private ephemeral JetStream ordered
pull consumer. Delivery is immediate, memory-backed, one replica, and ACK-free. It
does not reuse or advance any durable application's delivery/checkpoint state.

The native async-nats 0.50 ordered iterator batches 500 messages per pull, with
prefetching, a 30-second pull expiry and 15-second heartbeats. Its public API has
no configurable batch byte cap; the previous 8 MiB cap is not retained. Waiting
for each next message, including native recovery, has a five-second application
deadline. Absent history returns before creating a consumer.

The read stops at the captured cutoff. Directory-related history reads retain
their shared snapshot cutoff. Later concurrent appends, including invalid bytes,
are excluded before decoding. Existing event, commit, predecessor and checksum
checks are unchanged. ADR 0040's per-handle audit policy remains unchanged:
ordinary loads trust supported writes after local checks; strict handles and
explicit audits also verify historical transaction evidence. A truncated/inconsistent history
does not become a successful partial history.

The native iterator checks delivery sequences and recreates the consumer after
gaps, deletion or no responders, including after delivered events, resuming after
the last yielded stream sequence. Persistent failure returns retryable
unavailability at the application deadline. Native heartbeat recovery can exceed
that deadline. Stream/consumer identity and authoritative history checks remain.

Normal completion and validation failures drop the iterator and await consumer
deletion. A unique explicit ephemeral name is reused by native recreation and
cleanup; an already deleted consumer is clean. Dropping a cancelled read schedules
best-effort deletion; a 30-second server inactivity threshold also covers lost
creation replies and process/runtime failure. Cleanup failure prevents a
successful read and never masks an original validation error.

## Consequences

Healthy history retrieval costs one consumer creation/deletion and batched pulls,
not one request per historical event. Full replay still transfers and applies
the complete history. This is not aggregate snapshotting or a cross-request cache.
Explicit transaction receipt/guard auditing and append-suffix verification retain
individual raw lookups. Ordinary command-history loads no longer pay for either
per-event retrieval requests or historical receipt audits; strict/explicit audits
still have history-dependent evidence verification costs.

No authoritative storage format, stream policy, minimum NATS version, or durable
consumer configuration changes. Direct batch APIs would require enabling stream
direct access; ephemeral replay avoids silently changing operator-owned policy.

Readers now need scoped consumer-create, pull, and delete permissions in addition
to existing stream-info/raw-message and inbox permissions. Consumer/account limits
must admit concurrent replay cursors. Restrict grants to the application's event
stream (and its exact aggregate subject filters); do not grant broad management
permissions merely to enable replay. There is no silent per-event fallback if
these permissions or consumer capacity are missing.

## Verification

Real NATS coverage exercises batched pulls, large events, clustered recovery,
interleaved subjects, concurrent append cutoffs, cancellation cleanup, empty
histories and unchanged durable consumer progress. Existing event-store contracts
and transaction/snapshot corruption cases remain mandatory. Paired release benchmarks cover the old
single-event-commit workload, longer histories, and both trusted and audited
command-style transactions. The previous raw-window implementation and its tests
remain a test-only reference; they are not a production fallback.
