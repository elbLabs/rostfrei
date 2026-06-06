# ADR 0004: Private domain events and integration events

## Status

Accepted.

## Decision

Stored aggregate events are private domain facts and collectively form the
authoritative aggregate history. Their schemas evolve for permanent replay.
They are not transport notifications and are not published directly as public
contracts by the aggregate.

Integration events are bounded, independently versioned public messages.
Application code may derive them from committed private events, normally through
a projection or outbox boundary. Their addresses, compatibility policy, retry
policy, and consumers are independent from aggregate stream identity.

## Schema upgrades

Integration-event payload schema versions are independent from crate release
versions and durable consumer versions. NATS integration subjects do not include
the payload schema version, so consumers of the same address can receive both
old and new schemas.

Typed decoding accepts exactly the event type's `SCHEMA_VERSION`. The standard
`IntegrationEventCommandHandler` quarantines messages it cannot decode, including
unsupported schema versions. Changing a producer from v1 to v2 therefore requires
compatible consumers before v2 publication begins; changing only the consumer's
durable name does not select v2 messages.

For a rollout that keeps processing messages:

1. Deploy consumers that accept both old and new schemas through the
   `MessageHandler<IntegrationEventAddress>` port. Decode each supported schema
   into its corresponding event type and map it to the intended command.
2. Switch producers to the new schema after all affected consumers can read it.
3. Retain support for the old schema while old messages can still arrive through
   pending delivery, retries, retained history, or quarantine redelivery.

A v2-only typed handler is not sufficient for the first step: it would quarantine
remaining v1 messages. Applications own the compatibility mapping; the framework
does not automatically translate payload schemas.

See [ADR 0014](0014-application-scoped-nats-conventions.md#durable-consumer-versions)
for the separate replay consequences of changing a durable consumer version.

## Consequences

Changing a public contract does not rewrite aggregate history, and changing a
private domain model does not silently alter consumers. ADR 0016 adds a typed
integration-event bus and post-commit handler path. Publication can be retried
from durable domain-event consumption, but it is not atomic with event storage;
an outbox is still required where that stronger guarantee is necessary.
