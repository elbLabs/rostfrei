---
title: System
kind: reference
---

# System

## Definition

A **System** is the root boundary of one coherent domain model.

It is a model boundary, not a business entity or aggregate. It owns no business
state and performs no business action.

## Purpose

A System contains the [bounded contexts](bounded-context.md) that make up one
coherent business domain. It defines the boundary within which domain ownership
is resolved.

## Model Shape

Every System declares an identity:

```yaml
id: todo-system
label: Todo
```

The System declares its bounded contexts. Each [bounded context](bounded-context.md)
declares the domain objects and rules within its own boundary.

## Ownership

The System does not own [aggregates](aggregate.md), [entities](entity.md),
[value objects](value-object.md), [domain services](domain-service.md),
[decisions](decision.md), [actions](action.md), invariants, events, lifecycles,
or domain errors. Those artifacts are owned by the bounded context or domain
object that defines their meaning.

A System is complete when its bounded contexts and their owned artifacts form
one unambiguous domain model.

## Boundaries

A System does not define transport, storage, deployment, or provider
configuration. Those are platform concerns.

## Related Concepts

- [Bounded Context](bounded-context.md) partitions a System into domain
  ownership boundaries.
- [Aggregate](aggregate.md) defines a state and consistency boundary.
- [Domain Service](domain-service.md) coordinates aggregate behavior.
