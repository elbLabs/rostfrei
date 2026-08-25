# ADR 0001: Vocabulary and framework scope

## Status

Accepted.

## Decision

Zeitstrahl is event-sourced by design. An **aggregate stream** is the permanent,
ordered history of one aggregate type and identifier. A **domain event** is a
private, meaningful fact that occurred in that aggregate. A **commit** is the
atomic, non-empty batch of domain events produced by one accepted operation. A
**command** requests a decision. A **query** reads state and never appends to an
aggregate stream. An **integration event** is a separately versioned public
contract, normally derived from private domain events by application code.

The framework owns replay, versioning, pending events, execution metadata,
serialization boundaries, optimistic concurrency, and persistence adapters.
Aggregates own typed decisions and deterministic state transitions only.

Zeitstrahl does not provide conventional state persistence through its
aggregate abstraction. It also does not initially provide handler discovery,
procedural macros, workflows, reactions, projection orchestration, schema
generation, or execution journals.

## Consequences

Application code invokes an aggregate executor rather than a raw event-store
adapter. Product names, subjects, stream names, environment variables, and
deployment defaults do not belong in Zeitstrahl.
