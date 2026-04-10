# ADR 0002: Aggregate, codec, executor, and event-store boundaries

## Status

Accepted.

## Decision

An aggregate exposes an associated typed event and an `apply` transition. A
typed command handler records events through a decision context; recording an
event applies it immediately so later decisions in the same command observe the
new state. Pending events remain in the execution context, never in aggregate
state.

Aggregates do not implement Serde and do not receive wire envelopes, broker
headers, clocks, IDs, or storage handles. An `EventCodec` maps typed aggregate
events to and from bounded `NewEvent` and `RecordedEvent` values. Unknown event
types or schema versions and malformed payloads are explicit replay failures.

The executor owns load, strict history validation, decode, replay, command
handling, application of new events, encoding, expected-version append, exact
retry detection, and a bounded optimistic-concurrency retry loop. Rejected
commands append nothing.

The `EventStore` port exposes only aggregate-stream load and atomic append.
Adapters must implement the same observable behavior as the in-memory reference
store.

## Consequences

Domain tests can exercise typed behavior without NATS or Serde. Infrastructure
failures and domain rejections remain distinguishable. A command that performs
external side effects still needs a future execution-journal seam; the first
release deliberately does not pretend an event append makes external effects
atomic.
