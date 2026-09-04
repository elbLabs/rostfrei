# ADR 0016: Typed application buses own business message preparation

## Status

Accepted.

## Decision

`rostfrei` owns the application-facing `CommandBus`, `QueryBus`, and
`IntegrationEventBus`.
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
The handler-linked pairing and registration syntax are refined by
[ADR 0026](0026-handler-linked-commands.md): processors and typed bus calls name
both aggregate and command, while `CommandHandler<C> for A` supplies the sole
authored aggregate and rejection relationship.
`InMemoryMessagingAdapter` implements command and integration-event adapter
capabilities for local execution and contract tests.

ADR 0019 extends the same boundary to request/reply queries. `QueryBus` prepares
typed or dynamic query envelopes, while `QueryProcessor` owns typed handler
bindings and `InMemoryQueryAdapter` provides the local reference path.

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

Integration events are produced only from committed domain events. Application
code implements `IntegrationEventMapper` for each private event it publishes.
`IntegrationEventPublisher` supplies the `DomainEventHandler`, and
`register_integration_event` derives the private event registration from its
compiled definition. The source event identity defines the integration message
identity and causation, so retries are stable. Aggregate handlers never publish
integration events before commit.

A consuming bounded context implements `IntegrationCommandMapper` to map an
incoming public event to exactly one typed `IntegrationCommand<C>`. The mapped
value contains the command and its target aggregate ID and performs no messaging
I/O. `IntegrationEventCommandHandler` owns envelope validation, deterministic
command identity, correlation and causation propagation, command dispatch, and
delivery disposition. A terminal command response, including a business
rejection, acknowledges the integration event. Transient command-bus
unavailability retries it; malformed envelopes and invalid mappings are
quarantined. Independent mappings use independent durables and may each produce
their own command.

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

Application behavior is expressed as two typed transformations: domain event to
integration event in the publishing context, then integration event to targeted
command in the consuming context. Redelivery cannot duplicate a command for the
same mapping durable, while a second durable remains an independent mapping with
its own operation identity.
