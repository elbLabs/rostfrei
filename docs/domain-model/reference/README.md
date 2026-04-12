---
title: Model Engine Reference
kind: reference
---

# Model Engine Reference

This reference defines the target domain language. The handbook explains how
the concepts work together.

The Cargo package `rostfrei-domain` exposes the Rust crate name `domain`, so
examples use paths such as `domain::Aggregate`.

## Concepts

### Domain Boundaries

- [System](domain/system.md)
- [Bounded Context](domain/bounded-context.md)
- [Aggregate](domain/aggregate.md)
- [Entity](domain/entity.md)
- [Domain Identity](domain/domain-identity.md)
- [Value Object](domain/value-object.md)
- [Domain Service](domain/domain-service.md)

### Data Shapes

- [Custom scalar](domain/custom-scalar.md)

### Behavior

- [Action](domain/action.md)
- [Query](domain/query.md)
- [Domain Command](domain/domain-command.md)
- [Decision](domain/decision.md)
- [Invariant](domain/invariant.md)
- [Lifecycle](domain/lifecycle.md)

### Domain Outcomes

- [Domain Error](domain/domain-error.md)
- [Domain Event](domain/domain-event.md)

## Scope

The model defines business meaning. Transport, persistence, deployment, and
identity-provider setup remain platform concerns.
