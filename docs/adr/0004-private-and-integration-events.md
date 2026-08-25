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
private domain model does not silently alter consumers. The first release
provides the two sets of contracts but does not implement projection or outbox
orchestration.
