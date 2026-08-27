---
title: Bounded Context
kind: reference
---

# Bounded Context

## Definition

A **Bounded Context** is a named boundary for one coherent domain language.

It contains the [aggregates](aggregate.md) and
[domain services](domain-service.md) that share that language.

## Purpose

A bounded context prevents a term from having unclear or conflicting meaning
across a System.

For example, `Project` may mean a collaboration project in one context and a
delivery plan in another. They are separate concepts unless the model explicitly
connects them.

## Model Shape

```yaml
id: Todo
label: Todo management
```

## Ownership

A bounded context owns the domain boundary and may own shared Value Object
definitions, but it owns no business state.

- An [Aggregate](aggregate.md) owns its state and behavior.
- A [Domain Service](domain-service.md) coordinates aggregates in its bounded
  context.
- An [Entity](entity.md) belongs to an aggregate.
- A [Value Object](value-object.md) belongs to the bounded context, an aggregate,
  or an entity. A context-owned Value Object is shared only within that context.
- A [Decision](decision.md) belongs to an aggregate or entity. Any Action in this
  bounded context may call a Decision whose Rust function is visible; the
  compiler does not enforce call permissions.

## Boundaries

A bounded context does not directly:

- own aggregate state
- evaluate decisions independently of an Action
- invoke actions
- coordinate behavior across another bounded context

A domain service cannot coordinate aggregates from another bounded context.

Aggregate, Entity, and Value Object references may name only Aggregates in the
same bounded context.

## Related Concepts

- [System](system.md) contains bounded contexts.
- [Aggregate](aggregate.md) defines a consistency boundary within a bounded
  context.
- [Domain Service](domain-service.md) coordinates aggregate behavior within a
  bounded context.
