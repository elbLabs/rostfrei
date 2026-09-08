# ADR 0002: Aggregate, codec, executor, and event-store boundaries

## Status

Accepted. The aggregate-bound handler and primary transaction participant portions
are superseded by [ADR 0037](0037-bounded-context-commands-and-unit-of-work.md).

## Decision

An aggregate definition exposes associated state and event types plus an
`apply` transition. Initialization receives the stream identity, so state that
embeds its aggregate identity does not rely on `Default` or out-of-band state.
A typed application command handler loads aggregate instances through a command
unit of work. Raising an event applies it immediately so later actions and
decisions in the same command observe the new state. Each instance owns its
uncommitted events, while the domain root remains free of runtime bookkeeping.

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
direct commit or an event transaction. An event transaction has an ordered set
of unique aggregate-stream participants. A participant may contribute a commit
or act as a read-only expected-version guard, and at least one participant must
write. All participant commits share the transaction operation, fingerprint,
correlation, and causation metadata.

The operation identity addresses the durable transaction receipt within one
bounded-context event store; no participant is primary. Expected versions are
admission preconditions rather than durable transaction identity, so an
otherwise identical retry returns the original receipt after later commits. The
transaction limit counts every domain event, each read-only guard, and the
receipt against a common 100-item budget. A commit
contains at most 100 domain events. Commands that exceed either limit fail with
an actionable `InvalidRequest` before anything is appended.

Every `EventStore` implementation must provide durable transaction receipt
lookup and atomic transaction append. Adapters must implement the same observable
behavior as the in-memory reference store.

## Consequences

Domain tests can exercise typed behavior without NATS or Serde. Infrastructure
failures and domain rejections remain structurally distinct. A command that performs
external side effects still needs a future execution-journal seam; the first
release deliberately does not pretend an event append makes external effects
atomic.

An atomic event transaction cannot cross event stores. Cross-bounded-context or
cross-service consistency still requires an explicit process manager or saga.
