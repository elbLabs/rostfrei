# Rolling domain-event consumer handoff: worker-pool direction and pinning spike

**Historical investigation.** The selected worker-pool implementation and its
migration instructions are described in
[Rolling domain-event workers](domain-event-worker-pool.md). The pinning example
below remains a standalone protocol experiment.

Tracking: [#104](https://github.com/elbLabs/rostfrei/issues/104).
Related: [#81](https://github.com/elbLabs/rostfrei/issues/81) covers blocked-consumer
diagnosis and recovery.

## Goal

Allow application instances to overlap during a rolling deployment while bus
command handlers and domain-event publication remain in the same process.
Multiple instances share one durable's work and can be ready during the overlap.
Independent logical subscribers retain independent durable progress.

The preferred direction is a standard JetStream worker pool with explicit ACKs,
graceful shutdown, redelivery, and existing idempotency. The runnable pinning
example below records the earlier protocol investigation; it does not establish
that pinning is required for a worker pool.

### Queue groups and durable work sharing

The requested behavior is NATS-native work distribution. The linked
[Queue Groups guide](https://docs.nats.io/concepts/queue-groups#worker-design)
explicitly distinguishes live Core NATS delivery (at-most-once, no ACK/redelivery)
from [JetStream worker pools](https://docs.nats.io/learn/jetstream/worker-pool):
**one durable consumer, many workers pulling from it**. The latter preserves
stored event history and redelivers unacknowledged work after `AckWait`.

For this framework, retain the existing Limits-retention source stream and share
the durable between pod replicas. Allow multiple waiting pulls, ACK successfully
processed work explicitly, and stop starting new work during shutdown. Once local
processing has stopped, unfinished deliveries can be NAKed for earlier redelivery
or left unacknowledged for timeout-based recovery. A crash naturally uses the
timeout path. Pin IDs, pin expiry, and UNPIN are not prerequisites of this model.

The current domain-event loop still needs adaptation: it consumes one event and
then separately pulls the rest of that event's transaction, expecting those pulls
to stay with one worker. A worker pool distributes individual messages, not whole
stored transactions. Neither a queue name nor increasing `MaxWaiting` alone fixes
that assumption. Preserve complete-transaction validation, the ACK-after-success
rule, and source progression while designing a transaction-safe work unit or
reconstruction path from existing stored history. Do not silently weaken these
requirements or introduce new coordination records. This transaction-handling
change has not yet been implemented or validated by the pinning spike.

### Processing contract: at-least-once with stable identities

Preserve the existing ADR 0012 / ADR 0016 contract: handler effects are
at-least-once and idempotent by source event identity. Two attempts to publish
the same integration event during recovery are an expected retry case, not a
reason by themselves to introduce owner fencing or a publication journal.

The current framework already supplies the relevant identities:

- `integration_message_id()` in `crates/rostfrei/src/integration_event_bus.rs`
  derives the ID from the source event ID, target integration address, and schema
  version. Independent pods publishing the same mapping therefore use the same ID.
- `publish_confirmed()` in `crates/rostfrei-nats/src/publish.rs` sends that ID as
  `Nats-Msg-Id` and waits for PubAck. JetStream suppresses duplicate storage within
  the stream's duplicate window; the provisioning default is **two minutes**.
- `integration_operation_id()` in `crates/rostfrei/src/integration_event.rs`
  derives the downstream command's operation identity from the mapping durable
  and incoming message ID, reusing the existing command replay machinery.

Broker deduplication is time-bounded, and it matches IDs rather than checking
payload equivalence. Mappings must remain semantically consistent for the same
identity. Beyond the duplicate window, repeated bus records are possible;
downstream processing must retain its existing idempotency contract. This is not
an unlimited exactly-once guarantee for arbitrary subscribers or external effects.

### Persistence budget

Rolling-deployment coordination must add **no application-level stored messages
per source transaction or event**. Use native work distribution and the durable
consumer's existing ACK/redelivery state. Additional publication checkpoints, processing
receipts, or per-transaction KV cursor writes are outside this design's budget.

The proposed atomic output-plus-checkpoint approach was rejected because it adds
another stored record per transaction and publisher durable, on top of existing
operation receipts. Explicit ACKs and pinning still require protocol traffic and
broker-managed state; this constraint concerns additional application records,
not zero transport or storage overhead.

## Current problem

`NatsDomainEventConsumer` configures `max_waiting: 1`, `AckPolicy::All`, and a
pending-message limit large enough for a complete transaction. It pulls the first
event, then the transaction tail, validates the complete transaction, dispatches
its events in order, and double-ACKs the last delivery. Under `All`, this final
ACK cumulatively confirms earlier deliveries on the same durable.

An overlapping fast-inbox instance can encounter `409 Exceeded MaxWaiting` and
exit. The reported rollout then stalls: the new pod never becomes Ready,
Kubernetes retains the old pod, and Helm eventually times out. The waiting-request
limit is not an instance lock: between pulls, another instance can receive part
of a transaction. Raising the limit alone does not establish safe transaction
ownership.

## Upstream research

- [NATS ADR-42](https://github.com/nats-io/nats-architecture-and-design/blob/main/adr/ADR-42.md#pinned_client-policy)
  defines `group`, the optional pull `id`, `Nats-Pin-Id`, stale-ID `423` replies,
  the `priority_timeout` field, and the group-wide `CONSUMER.UNPIN` API.
- [Go client pull implementation](https://github.com/nats-io/nats.go/blob/main/jetstream/pull.go)
  stores the ID on the consumer instance, attaches it to subsequent pulls, and
  clears it on pin mismatch. The spike uses the same protocol through
  `async_nats::jetstream::Context::{request, send_request}`.
- [Go client `TestConsumerPinned`](https://github.com/nats-io/nats.go/blob/main/jetstream/test/consumer_test.go)
  exercises active/standby clients. Its shutdown tests also account for queued
  pulls whose inbox has disappeared: stopping a local subscription is not an
  immediate server-side cancellation of outstanding pulls.
- A client acquires its pin on actual delivery, not simply by connecting. Startup
  on an empty stream must therefore not require a pin before reporting readiness.
- [async-nats 0.50 policy definition](https://docs.rs/async-nats/0.50.0/async_nats/jetstream/consumer/enum.PriorityPolicy.html)
  explicitly says `PinnedClient` is not implemented by the client. Its presence
  in the enum is for configuration deserialization. The inspected upstream main
  branch has the same limitation. The Rust config also lacks `priority_timeout`.
- ADR-42 specifies explicit ACKs for pinned consumers. This is the intended
  contract even where a server version's validation does not reject every other
  ACK policy.

### Server versions

Pinning was introduced in NATS **2.11.0**. Rostfrei's event-store minimum is
already **2.12.1**, so the feature does not inherently require a new minor series.
However, [2.12.5](https://github.com/nats-io/nats-server/releases/tag/v2.12.5)
contains relevant handoff fixes in
[nats-server#7819](https://github.com/nats-io/nats-server/pull/7819):

- Administrative unpin wakes waiting clients without requiring a new publication.
- Consumer leadership changes reset pinned state.
- Administrative unpin correctly clears the pin timer/state.

[2.12.8](https://github.com/nats-io/nats-server/releases/tag/v2.12.8) also refactors
pin-header handling. **2.12.15 is the proposed test/deployment baseline for this
spike**, matching the native NATS authentication fixture already used by CI.
A move to 2.14 or 2.15 is not a prerequisite.

The inspected local `lococo-gitops` overlays configure three NATS servers and
replication factor three for production, and one server / replication factor one
for staging and ephemeral branches. The accompanying Nexus chart pins the NATS
subchart to 2.12.4. Inspecting that upstream chart archive confirms its default
server image is `nats:2.12.4-alpine`; the inspected values do not override it.
These are checked-in configuration findings, not a live-cluster inventory.

These version findings concern the pinning spike. Ordinary shared-durable pull
workers do not require the pinning-specific 2.12.5 fixes or missing Rust pin
support. The framework must declare its actual supported baseline and test both
single-server and replicated consumers. Application authors should not need to
implement their own handoff or select a correctness strategy for each topology.

### Observed results (2026-10-08)

| Scenario | NATS 2.12.1 | NATS 2.12.15 |
| --- | --- | --- |
| Standby waiting between transaction pulls; administrative handoff of existing backlog | Timed out after UNPIN, without a fresh publication | Passed |
| ACKing the last event under Explicit leaves the earlier two pending | Not exercised | Passed |
| Takeover after only the first event's ACK; redelivery of the remaining suffix | Not exercised | Passed |
| TTL takeover, stale-pull rejection, and acceptance of an old delivery's ACK | Not exercised | Passed |

The existing-backlog handoff test was also run against **2.12.4**, matching the
inspected chart's default server image, and timed out after UNPIN in the same way
as 2.12.1.

The last test is a limitation test: a successor received event two while event
one was still unacknowledged, and NATS accepted event one's old ACK after pin
loss. Passing this test documents actual broker behavior, not exclusive effects.

The `rostfrei-nats` package suite passed on 2.12.15: 193 tests and one compile
doctest, with five existing native-server/TLS fixtures ignored by the default
command. The local full-suite run used disposable Docker tmpfs storage because
the host's free disk capacity was below a legacy test's 10 GiB stream reservation.
The four prototype tests also passed using the ordinary broker runner. Package
Clippy (all targets/features), formatting, and the 21 Python runner/version tests
passed.

## Executable pinning example

The example creates and removes a uniquely named, bounded, in-memory stream.
Its two readers use separate NATS connections and the same durable. It provisions
`explicit`, one priority group, `max_waiting: 16`, and `max_ack_pending: 100`.
The values are illustrative, not a production capacity recommendation.

```sh
python3 scripts/test_nats.py -- \
  cargo run --locked -p rostfrei-nats --example pinned_handoff

python3 scripts/test_nats.py -- \
  cargo test --locked -p rostfrei-nats --example pinned_handoff -- --nocapture
```

The wire adapter:

1. Uses a fresh inbox for each finite pull and tracks the pin across pulls.
2. Sends the group on every pull and the ID once assigned.
3. Treats `408` as normal expiry, known stale-ID `423` replies as pin loss, and
   only `409 Exceeded MaxWaiting` as contention. Other statuses are errors.
4. Returns partial batches together with their completion status; callers must
   not process incomplete transactions or interpret pin loss as success.
5. Double-ACKs each event in order only after the simulated transaction succeeds.
6. Uses a coordinator-controlled administrative unpin for the handoff demo.

Coverage includes a standby waiting between the first-event and tail pulls,
handoff of already-published backlog, explicit-ACK scope, recovery after a
partially acknowledged batch, and TTL takeover with stale-pull rejection.

The sample payloads label illustrative transaction groups. The example is a
protocol demonstration; integrating actual event reconstruction, receipt
validation, handlers, retry classification, readiness, and shutdown into
`NatsDomainEventConsumer` is the next stage.

### Earlier pinned integration candidate

The following path applies if pinning is selected to retain delivery affinity.
The preferred shared-durable worker-pool design above first needs to resolve the
current transaction-tail pull assumption; these steps do not validate that change.

1. Share one pinned durable between replicas, keep pin state per instance, and
   allow a healthy standby to report readiness. Treat normal contention, pull
   expiry, and stale-pin replies as lifecycle transitions.
2. Preserve the existing earliest-unresolved-event check before dispatch. A new
   pin does not authorize skipping an unacknowledged source transaction: defer a
   later delivery and wait for the unresolved work to complete or be redelivered.
3. Reconstruct and validate the complete transaction, including an acknowledged
   prefix after partial explicit ACK completion. Publish mapped events in source
   order with the existing stable message identities and wait for each PubAck.
4. Only after the entire transaction succeeds, explicitly ACK each live delivery
   in order. A duplicate PubAck is successful publication. A late ACK from the
   previous worker is acceptable under the same success rule.
5. During shutdown, stop starting new transactions and finish in-flight work
   within the termination budget. Bound processing/pull/ACK deadlines and return
   to standby on detected pin loss; resolve safe fast release separately from
   timeout-based crash recovery.
6. Validate the actual publisher with a delayed old worker, a successor replaying
   the same transaction, and later transactions waiting behind unresolved work.
   Cover retries both inside and outside the configured duplicate window.

## Limits that affect the design

### Pinning does not fence processing or ACKs

ADR-42 explicitly disclaims exclusive processing. A pin can expire while the old
instance is still working. The next client can receive a later message while an
earlier one remains unacknowledged, and an old delivery's ACK is not conditional
on the current pin ID. A stale pull gets `423`; already running effects are not
cancelled by that response.

The TTL test deliberately checks these limits rather than claiming exactly-once
processing or a globally atomic handoff. Acceptance of an old ACK is not itself
a correctness failure: if that worker successfully completed the whole
transaction before ACKing, the source work has been done. Explicit ACKs scope
confirmation to the individual source messages rather than unrelated outstanding
deliveries.

Duplicate suppression and ordering address different cases. Repeating event A
can reuse A's message ID; delivering event B while A has never been published
cannot be repaired by deduplication. Preserve the earliest-unresolved-event guard,
complete-transaction reconstruction, sequential publication, and PubAck-before-
source-ACK discipline. The raw TTL test bypasses these application-level checks.
Prove their interaction with pin loss and partial ACK recovery in the actual
consumer before enabling the mode. Broker delivery exclusivity and duplicate-free
physical bus history after the deduplication window are not the existing contract.

### Administrative unpin is not an ownership-checked release

`CONSUMER.UNPIN` accepts a group, not an expected pin ID. Calling it blindly from
each worker's shutdown or drop path can unpin a successor after this worker has
already lost its pin. Checking consumer info first does not make that operation
atomic. The example exposes it as a separate administrative function and does
not call it from `Drop`.

Decide whether normal shutdown relies on TTL takeover, uses a coordinated
administrative handoff, or needs upstream support for conditional unpin.

The framework should own this policy. The intended user-facing behavior is a
prompt graceful handoff after draining and confirmed ACKs, with timeout-based
recovery after a crash. A timeout-only first implementation must state its normal
deployment pause explicitly. For example, a 30-second pin lifetime can leave
nearly 30 seconds without new delivery after the last active pull. Outstanding
source deliveries may additionally need redelivery after their ACK wait expires.
The pin timeout alone does not bound total recovery time. Any fast-release path
must address the unconditional-UNPIN race described above.

### ACK and pin deadlines cover different intervals

`AckWait` begins when a message is delivered. The pin timeout is refreshed by a
pull, not by handler progress or ACKs. Loading/validation, handler execution,
retry delays, and explicit ACK round trips must fit the applicable deadlines or
use a deliberately designed renewal strategy. Increasing a timeout trades faster
failover for a larger processing budget; it does not create fencing.

### Existing consumers need migration

NATS does not permit updating `AckPolicy` or `MaxWaiting` in place (see
[server validation](https://github.com/nats-io/nats-server/blob/v2.12.15/server/consumer.go)).
The existing `All`/one-waiter durable must be replaced through a controlled
handoff that preserves the confirmed stream position. The current provisioner
uses `DeliverPolicy::All`; blindly recreating it would replay the whole matching
history. If the pinned variant is selected, old instances also do not participate
in its pin protocol.

## Open decisions before production integration

1. Make transaction reconstruction/processing compatible with work distributed
   across a shared durable. Validate complete-transaction handling and ordered
   progress without relying on affinity across separate pulls.
2. Implement framework-owned graceful shutdown and crash recovery with documented
   timing and validated ACK/pull/processing timeout defaults. Include explicit
   NAK after stopping unfinished work and timeout redelivery after a crash.
3. Validate the existing at-least-once contract with stable identities and the
   unresolved-event guard. Overlapping retry attempts alone do not require
   fencing; skipping unresolved work is a correctness failure that must be
   prevented within the persistence budget.
4. Establish the server support matrix and single-node / replicated-consumer
   coverage for the selected design. Keep pinning-specific client support and
   UNPIN questions conditional on actually choosing the pinned variant.
5. Specify the one-time durable migration, rollback, and restart position.
6. Expose waiting/processing/draining/blocked states so a healthy waiting worker
   can be Ready while genuinely blocked publication remains observable.
7. Validate multi-aggregate transactions, publication identities, redelivery,
   connection loss, stale workers, and replicated-consumer leader changes using
   the real domain-event pipeline, beyond this single-server protocol spike.

## Production acceptance criteria

- [ ] Add no application-level stored messages or KV cursor writes per transaction
      or event for rolling-deployment coordination.
- [ ] Specify an ownership-loss and shutdown contract that preserves the required
      source transaction progression and at-least-once effects within the
      persistence budget, allowing idempotent retries of the same work.
- [ ] Integrate shared-durable work distribution with actual reconstruction,
      receipt validation, handler execution, retry classification, and explicit
      ACKs. Demonstrate progress when separate workers receive events from the
      same transaction.
- [ ] Demonstrate idle and busy rolling overlap without fatal contention or split
      transaction handling, with standby compatible with readiness.
- [ ] Verify crash recovery after partial ACK completion and stable identities
      after partially successful integration-event publication.
- [ ] Delay an old publisher until after the successor publishes the same source
      event; verify duplicate PubAck within the window and idempotent downstream
      command handling after the window, with consistent mappings.
- [ ] Verify that a worker receiving a later event waits for unresolved earlier
      work instead of dispatching it out of order, without deadlocking workers
      that hold different portions of the same transaction.
- [ ] Exercise connection loss and replicated-consumer leadership changes.
- [ ] Document permissions, server baseline, migration with preserved progress,
      rollback, and acceptable handoff latency.
