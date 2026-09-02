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

## Consequences

Changing a public contract does not rewrite aggregate history, and changing a
private domain model does not silently alter consumers. ADR 0016 adds a typed
integration-event bus and post-commit handler path. Publication can be retried
from durable domain-event consumption, but it is not atomic with event storage;
an outbox is still required where that stronger guarantee is necessary.
