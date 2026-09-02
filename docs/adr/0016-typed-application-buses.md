# ADR 0016: Typed application buses own business message preparation

## Status

Accepted.

## Decision

`rostfrei` owns the application-facing `CommandBus` and `IntegrationEventBus`.
They derive addresses from a bounded context, encode canonical JSON envelopes,
propagate correlation and causation, enforce payload bounds, and derive stable
content fingerprints and message identities. Applications submit typed command
and integration-event values. Dynamic command requests remain available for
explicitly registered tooling and transport boundaries, but they enter the same
bus and processor path.

`CommandProcessor` owns envelope validation, command registration, typed payload
decoding, stream and execution-metadata construction, aggregate execution, and
stable accepted or rejected command responses. Adding a command registers a
typed binding; processors and adapters do not branch on command-name strings.
`InMemoryMessagingAdapter` implements command and integration-event adapter
capabilities for local execution and contract tests.

`rostfrei-messaging-core` continues to own validated addresses, envelopes,
metadata values, response and rejection contracts, and low-level publishing and
consumption ports. It does not know typed commands, aggregate execution,
or a broker.

`rostfrei-nats` implements the erased command and integration-message adapter
capabilities. It owns JetStream PubAck retries, durable command-response reads,
response reconciliation, response-before-ACK ordering, consumer dispositions,
and broker-specific failure translation. It receives already encoded messages
and does not contain application command matching or domain rejection mapping.

`rostfrei-tracer` adapts authorized external JSON Test and Dispatch requests to
the dynamic entry point on `CommandBus`. Tracer observes publication and
terminal responses, but it is not a privileged executor and does not define a
separate command wire contract.

Integration events are produced only from committed domain events. A
`DomainEventHandler` maps a concrete committed event to a public integration
event and publishes it through `IntegrationEventBus`. The source event identity
defines the integration message identity and causation, so retries are stable.
Aggregate handlers never publish integration events before commit.

## Consequences

The same typed application API runs against in-memory and NATS adapters without
changing business code. Canonical encoding and stable identities give both
adapters equivalent deduplication and replay semantics. Transport concerns stay
outside aggregate execution, while external tools retain a bounded dynamic
entry point through the same validation and response path.

Post-commit publication does not make event storage and integration publication
atomic. Durable domain-event consumption retries failed publication and stable
message identities make replay safe, but deployments that require a stronger
atomic guarantee still need an outbox or equivalent journal.
