# ADR 0002: Aggregate, codec, executor, and event-store boundaries

## Status

Accepted.

## Decision

An aggregate definition exposes associated state and event types plus an
`apply` transition. Initialization receives the stream identity, so state that
embeds its aggregate identity does not rely on `Default` or out-of-band state.
A typed command handler receives an `AggregateInstance`. Raising an event applies
it immediately so later actions and decisions in the same command observe the
new state. The instance owns its uncommitted events, while the domain root remains
free of runtime bookkeeping.

Aggregates do not implement Serde and do not receive wire envelopes, broker
headers, clocks, IDs, or storage handles. An `EventCodec` maps typed aggregate
events to and from bounded `NewEvent` and `RecordedEvent` values. Unknown event
types or schema versions and malformed payloads are explicit replay failures.
In the compiled-model path, the canonical `Aggregate` derive generates the
aggregate-wide event representation from its attached concrete domain events.
Applications raise those concrete events and do not declare that
representation. The generated representation uses JSON automatically; each
payload supplies its stable event ID and schema version and implements Serde.
Applications provide an explicit `EventCodec` only when they need custom DTOs,
legacy schemas, upcasting, or a non-JSON format. Manual aggregate event types and
codecs remain available to direct `rostfrei-core` users.

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
