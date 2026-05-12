# ADR 0012: Durable post-commit domain-event handlers

## Status

Superseded for application-facing use by
[ADR 0016](0016-typed-application-buses.md). Retained as an infrastructure
contract.

## Decision

`DomainEventHandler` is the low-level infrastructure port used to adapt
committed private domain events to a durable post-commit processor. It is not an
application extension point. Application code uses dedicated typed
transformations such as `IntegrationEventMapper`; framework adapters implement
the handler, publication, retry, and failure-classification mechanics.

Infrastructure registers a private event type and framework handler. The
derived JSON event codec is selected automatically; registration accepts an
explicit codec only as an override. Unregistered aggregate/event pairs are intentionally irrelevant and
are treated as successfully handled without invoking a side effect. Registered
events are decoded through the aggregate's codec; unsupported schemas, malformed
payloads, permanently unsupported events, and operator-blocking failures stop
the durable consumer without skipping the event. Retryable failures cause the
complete atomic transaction group to be negatively acknowledged for redelivery.

The NATS adapter consumes the authoritative EventStore JetStream stream through
caller-named durable pull consumers. It verifies the existing stream and durable
configuration without provisioning at service startup. It validates and buffers
one complete ADR-50 atomic transaction group before dispatch, reconstructing
filtered progress across internal transaction guards and receipts when needed.
It invokes handlers in transaction order, ACKs the group as a unit only after
every invocation succeeds, and retries the group as a unit after a retryable
failure or timeout. Limits retention keeps aggregate replay, other durables,
future rebuilds, and permanent history independent from consumer
acknowledgements.

Independent side effects use independent durable consumers. Integration-event
publication and future read-model processing use dedicated application-facing
mapping contracts backed by framework `DomainEventHandler` adapters.
NATS messages, subjects, headers, ACK handles, and broker sequence values are
never exposed to application handlers.

## Consequences

Framework handler effects are at-least-once and must be idempotent by committed
event identity. Public integration-event publication waits for its PubAck before the
domain-event delivery is acknowledged. Poison events block their durable until
an operator repairs the cause or makes an explicit skip decision; rostfrei
does not automatically quarantine and continue an incomplete projection or
public history.
