---
title: Owned State Change
kind: handbook
---

# Owned State Change

An [Action](../reference/domain/action.md) changes only state owned by its
domain object.

```text
Action Decisions
  -> Stage Owned State Change
  -> Explicit Invariant Validation
```

Owned State Change produces the completed candidate; it does not commit that
candidate. The owning Action explicitly validates, translates any complete
violation collection, and commits only on success.

## Aggregate

An [Aggregate](../reference/domain/aggregate.md) Action may:

- change Aggregate state
- create or remove its contained [Entities](../reference/domain/entity.md)
- apply results returned by directly owned Entity or
  [Value Object](../reference/domain/value-object.md) Actions

It does not change another Aggregate's state.

In the conceptual runtime flow, an Aggregate that creates an Entity initializes
that Entity's optional Lifecycle. The current Rust compiler does not bind a
lifecycle state field or generate this initialization. Independently, the Entity
Action explicitly validates the completed Entity candidate, and the Aggregate
Action then explicitly validates the completed Aggregate candidate.

An Aggregate removes an Entity structurally. An Entity has no separate delete
Action.

## Entity

An Entity Action may change only its own existing state.

It may apply a replacement Value Object returned by one of its owned Value
Object Actions.

It does not create, remove, or change another Entity.

## Value Object

A Value Object Action returns a new value.

It does not change persisted state directly.

## Domain Service

A [Domain Service](../reference/domain/domain-service.md) does not change state
directly.

It coordinates public Aggregate Actions that perform their own owned state
changes.

## Boundaries

An owned state change does not:

- commit state
- emit a [Domain Event](../reference/domain/domain-event.md)
- change another owner's state
- bypass a delegated child Action
