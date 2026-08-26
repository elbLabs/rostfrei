---
title: Delegation
kind: handbook
---

# Delegation

Delegation is the domain-flow step where an [Action](../reference/domain/action.md)
asks an allowed owner to perform its own behavior.

```text
Aggregate Action
  -> directly owned Entity Action
  -> directly owned Value Object Action

Entity Action
  -> directly owned Value Object Action

Domain Service Action
  -> public Aggregate Action
```

No other delegation is allowed.

## Child Flow

A delegated Action runs its complete owned flow.

```text
Delegated Action
  -> optional Lifecycle Admission
  -> visible Action Decisions
  -> local state or value change
  -> explicit local Invariant Validation
  -> internal result
```

The parent does not run the child Action's Decision calls or
[Invariants](../reference/domain/invariant.md). The compiler does not inject
validation or rollback into the child Action. The child Action may call any
visible [Decision](../reference/domain/decision.md) in the same Bounded Context;
the Decision need not have the child's owner. Decision calls are ordinary Rust
calls, not delegation, and compiler call permissions are not enforced.

## Allowed Result

An allowed internal child Action returns its typed output and its owned state or
value change.

The parent may use that output without inspecting the child Action's Decision
outputs or state representation.

## Denied Result

A denied child Action stops its parent’s behavior.

```text
Value Object denial
  -> Entity returns internal denial
  -> Aggregate translates to Aggregate Domain Error

Entity denial
  -> Aggregate translates to Aggregate Domain Error

Aggregate denial
  -> Domain Service translates to Service Domain Error
```
