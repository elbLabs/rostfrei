# ADR 0013: Durable post-commit domain-event handlers

## Status

Accepted.

## Decision

`DomainEventHandler` is the one framework abstraction for application and
infrastructure side effects caused by committed private domain events. A handler
receives a typed event together with its existing `RecordedEvent` metadata. It
runs after aggregate commit, never participates in aggregate decisions, and
never changes or appends to the originating aggregate stream.

Applications explicitly register an aggregate codec, private event type, and
handler. Unregistered aggregate/event pairs are intentionally irrelevant and are
acknowledged without invoking a side effect. Registered events are decoded
through the aggregate's `EventCodec`; unsupported schemas, malformed payloads,
permanently unsupported events, and operator-blocking failures stop the durable
consumer without skipping the event. Retryable failures are negatively
acknowledged for redelivery.

The NATS adapter consumes the authoritative EventStore JetStream stream through
caller-named durable pull consumers. It verifies the existing stream and durable
configuration without provisioning at service startup. It validates and buffers
one complete ADR-50 commit before dispatch, invokes handlers in commit order,
and advances that durable only after every handler invocation succeeds. Limits
retention keeps aggregate replay, other durables, future rebuilds, and permanent
history independent from consumer acknowledgements.

Independent side effects use independent durable consumers. Publishing an
integration event and updating a read model are application-specific
`DomainEventHandler` implementations, not separate framework handler kinds.
NATS messages, subjects, headers, ACK handles, and broker sequence values are
never exposed to application handlers.

## Consequences

Handler effects are at-least-once and must be idempotent by committed event
identity. Public integration-event publication waits for its PubAck before the
domain-event delivery is acknowledged. Poison events block their durable until
an operator repairs the cause or makes an explicit skip decision; rostfrei
does not automatically quarantine and continue an incomplete projection or
public history.
