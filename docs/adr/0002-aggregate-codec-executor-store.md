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

Execution returns `CommandResult<Rejection>`. A completed business decision is
either `CommandOutcome::Accepted(CommandReceipt)` or
`CommandOutcome::Rejected(Rejection)`. Accepted receipts distinguish appended
events, an exact replay of previously appended events, and an accepted decision
that produced no events. Only codec and EventStore failures are returned as
`CommandExecutionError`; a modeled rejection is not an execution error.

The `EventStore` port loads aggregate streams and atomically appends either one
commit or an event transaction. An event transaction has an ordered set of
unique aggregate-stream participants. A participant may contribute a commit or
act as a read-only expected-version guard, but the first, primary participant
always contributes a commit. All participant commits share the transaction
operation, fingerprint, correlation, and causation metadata.

The first participant is the primary stream. Its stream identity and the
operation identity address the durable transaction receipt used for exact
retry. Expected versions are admission preconditions rather than durable
transaction identity, so an otherwise identical retry returns the original
receipt after later commits. The transaction limit counts every domain event,
each read-only guard, and the receipt against a common 1,000-item budget.

Adapters that do not implement multi-stream transactions retain a default
single-stream adapter over `append`. All adapters must implement the same
observable behavior as the in-memory reference store within their declared
capabilities.

## Consequences

Domain tests can exercise typed behavior without NATS or Serde. Infrastructure
failures and domain rejections remain structurally distinct. A command that performs
external side effects still needs a future execution-journal seam; the first
release deliberately does not pretend an event append makes external effects
atomic.

An atomic event transaction cannot cross event stores. Cross-bounded-context or
cross-service consistency still requires an explicit process manager or saga.
