# ADR 0039: Durable acceptance without domain events

## Status

Accepted. Extends ADRs 0003 and 0037. Implements issue #84.

## Decision

Every successful `CommandExecutor::execute` persists acceptance before returning,
including commands that emit no domain events. This is the default guarantee,
with no opt-out policy. A successful event-free decision is a legitimate business
outcome and does not require a fabricated event.

`CommandReceipt` distinguishes:

- `Appended(events)`: newly committed domain events and acceptance.
- `AcceptedNoEvents`: newly persisted acceptance with no domain events.
- `ExactReplay(events)`: the original durable acceptance. An empty event list
  replays event-free acceptance. The handler is not invoked when the receipt is
  found at execution entry.

The former transient `NoEvents` variant is removed. Read-only `simulate` returns
its separate `SimulationOutcome`; it never persists acceptance. Ordinary domain
rejections still do not create event-store receipts. Retained transport responses
are a separate concern.

## Identity and concurrency

Acceptance uses the existing `(bounded context, OperationId)` receipt namespace.
The operation fingerprint, correlation and causation must match the accepted
command. Changed evidence returns the normal `IdentityConflict`, including when
the winning command emitted no events. Applications remain responsible for a
stable, complete command fingerprint; the command processor computes it from
the registered command identity, payload and execution provenance.

The receipt write is atomic with all domain events and read guards. Concurrent
attempts under one identity have one winner, independent of broker deduplication.
Matching attempts replay that winner; changed-evidence contenders cannot both
be accepted. If simultaneous executions derive different participants or events
from different states, the executor reconciles the winning receipt using command
metadata before retrying or returning a conflict. This also handles a race between
event-free acceptance and an event-producing commit. The low-level transaction
API retains stricter participant/content equality for exact append retries.

Different operation identities racing to register the same customer can produce
one eventful winner and one event-free acceptance after optimistic retry. Both
operations acquire independent receipts, so subsequent changed commands conflict
under either identity.

## Read consistency and failure

Every loaded aggregate without events is a read guard, even when there are no
writing participants. All expected versions must hold at acceptance's atomic
commit point. Stale guards cause `Conflict` and a bounded, fresh-session rerun of
the entire handler. No acceptance is written for a failed attempt. Later changes
to a guarded aggregate do not invalidate a committed receipt or cause replay to
rerun the decision.

Commands loading no aggregates persist a receipt-only transaction. The atomic
item limit includes one receipt and one item per read guard: at most 99 guards
with the current 100-item limit. Aggregate histories and domain versions advance
only for actual domain events; guards do not create aggregate-directory entries.

Unavailable storage, capacity exhaustion or an unconfirmed receipt write cannot
return durable acceptance. An uncertain NATS publish may succeed only after
reconciliation proves the receipt was committed. The command worker retries
retryable persistence failures without publishing an accepted response or ACKing
the delivery as accepted.

## Storage and compatibility

The memory reference store applies receipt insertion and version checks under
the same lock. Its receipts survive recreation of executors/processors over the
same shared state, with the same lifetime as its in-memory event histories.

NATS stores event-free acceptance on the existing operation-derived receipt
subject. Optional guard records and the receipt are one atomic batch, with a
zero-last-subject-sequence expectation on the receipt subject. Receipt-only
commands use a one-item atomic batch. No control record is published on an
aggregate-event subject, so domain-event consumers, post-commit business handlers
and integration-event mappers do not receive fictitious business facts.

Eventful receipt schema 1 and event schemas 1–4 retain their formats and validation.
Event-free receipts use schema 2 with the same checksummed metadata and zero or
more read-only participants. New readers accept both receipt schemas; old readers
cannot replay schema-2 receipts. Upgrade command workers sharing an event store
together. No stream-policy migration or history rewrite is required.

Receipts and guards share the authoritative append-only event store's retention
and capacity policy. They outlive transport response retention, client/process
restarts, broker restarts, and broker duplicate windows. Storage capacity must
account for event-free traffic as well as domain events.

Past `NoEvents` acceptances have no stored evidence and **cannot be reconstructed
retroactively**. Retrying such an operation after upgrading may execute it again;
only a newly persisted acceptance establishes the enhanced guarantee. Low-level
direct append identities remain in their documented, separate stream namespace.

## Verification

Shared memory/NATS contracts exercise empty and guarded transactions, stale reads,
exact retries, changed metadata, same-operation contention and first-registration
losers. A Docker-backed acceptance test runs fresh application processes before
and after broker restart, proves expiry using a broker deduplication probe, and
checks replay through a recreated adapter/processor/store connection. It also
fills a real event store and verifies that receipt failure yields worker retry
without an accepted response or acceptance receipt.
