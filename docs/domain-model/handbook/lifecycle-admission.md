---
title: Lifecycle Admission
kind: handbook
---

# Lifecycle Admission

## Implementation Status

This page describes a conceptual runtime flow, not compiler-generated behavior.
The Rust compiler currently emits Entity lifecycle metadata only; it does not
bind current state, wrap Actions, or execute admission. See the
[Lifecycle implementation boundary](../reference/domain/lifecycle.md#implementation-status).

Lifecycle Admission decides whether an [Action](../reference/domain/action.md)
may begin in its owner's current [Lifecycle](../reference/domain/lifecycle.md)
state when an application implements that flow.

```text
Domain Action Call
  -> Lifecycle Admission
  -> Action Decisions
```

An application-provided admission step checks only the Lifecycle of the
Action's own owner.

```text
Conceptual application flow

Aggregate Action
  -> application-provided Aggregate Lifecycle admission

Entity Action
  -> application-provided Entity Lifecycle admission

Value Object Action
  -> no Lifecycle

Domain Service Action
  -> no Lifecycle
```

Compiler projection only supplies Entity lifecycle metadata; it does not connect
that metadata to Entity Action calls.

When an owner has no Lifecycle, Lifecycle Admission is skipped.

## Allowed

When the Lifecycle allows the Action, the Action continues to any explicit
[Decision](../reference/domain/decision.md) calls in its behavior. Those
Decisions may belong to any owner in the same Bounded Context when their Rust
contracts are visible.

No state changes or [Domain Events](../reference/domain/domain-event.md) occur
during Lifecycle Admission.

## Denied

When a public Aggregate Action is not allowed, the Action returns its owned
[Domain Error](../reference/domain/domain-error.md).

When an internal Entity Action is not allowed, its parent Aggregate translates
the denial before returning a public result.
