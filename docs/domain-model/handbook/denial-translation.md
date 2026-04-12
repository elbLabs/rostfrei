---
title: Denial Translation
kind: handbook
---

# Denial Translation

Denial Translation converts an internal denial into the
[Domain Error](../reference/domain/domain-error.md) owned by the current public
boundary.

```text
Internal denial
  -> Denial Translation
  -> Denied Action Result
```

## Aggregate Boundary

An [Aggregate](../reference/domain/aggregate.md) translates:

- its own Action denial selected from Decision output or lifecycle results
- the complete `Vec<InvariantViolation>` returned by owner validation
- directly owned [Entity](../reference/domain/entity.md) denials
- directly owned [Value Object](../reference/domain/value-object.md) denials

into an Aggregate-owned Domain Error.

```text
Value Object denial
  -> Entity internal denial
  -> Aggregate Domain Error

Entity denial
  -> Aggregate Domain Error
```

Several internal denials may map to one reusable Aggregate Domain Error. For
invariant validation, the Action translates the complete deterministic
violation collection, not only the first failure. Its Aggregate-defined
`violations` details distinguish the local causes.

The Aggregate exposes Aggregate-defined fields, details, and violation paths. It
does not expose internal error identities, action names, decision names, or
object structure.

## Domain Service Boundary

A [Domain Service](../reference/domain/domain-service.md) translates:

- its own Action denial selected from Decision output
- a delegated Aggregate Domain Error

into a Service-owned Domain Error.

Several Aggregate denials may map to one reusable Service Domain Error. Its
Service-defined `violations` details distinguish the local causes.

## Technical Failure

A technical failure is not translated.

It remains a technical failure and no state change or
[Domain Event](../reference/domain/domain-event.md) occurs.
