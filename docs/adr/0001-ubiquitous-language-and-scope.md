# ADR 0001: Ubiquitous language and framework scope

## Status

Accepted.

## Decision

rostfrei is event-sourced by design. The definitions in this ADR and the
project's `UBIQUITOUS_LANGUAGE.md` are its ubiquitous language. Framework APIs,
architecture decisions, documentation, Studio, and AI tooling use these terms
consistently rather than introducing local synonyms.

An **aggregate** is a business consistency boundary identified by its aggregate
type and aggregate ID. An **aggregate stream** is the permanent, ordered history
of one aggregate identity. **Aggregate state** is reconstructed by replaying
that history; it is not a separately persisted source of truth.

A **command** requests a deterministic **decision** from an aggregate. A
**rejection** is an expected business outcome that appends nothing. A **domain
event** is a private, meaningful fact that occurred in that aggregate. A
**commit** is the atomic, non-empty ordered set of domain events produced by one
accepted operation.

A **query** reads state and never appends to an aggregate stream. An
**integration event** is a separately versioned public contract, normally
derived from private domain events by application code after commit.

The framework owns replay, versioning, pending events, execution metadata,
serialization boundaries, optimistic concurrency, and persistence adapters.
Aggregates own typed decisions and deterministic state transitions only.

rostfrei does not provide conventional state persistence through its
aggregate abstraction. It also does not initially provide handler discovery,
procedural macros, workflows, reactions, projection orchestration, schema
generation, or execution journals.

## Consequences

Application code invokes an aggregate executor rather than a raw event-store
adapter. Product names, subjects, stream names, environment variables, and
deployment defaults do not belong in rostfrei. Documentation and APIs qualify
the word `event` as domain event or integration event, and qualify `stream` as
aggregate stream or JetStream stream whenever the shorter term would be
ambiguous.
