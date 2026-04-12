---
title: Lifecycle Conformance
kind: handbook
---

# Lifecycle Conformance

## Implementation Status

This page describes a conceptual runtime flow, not compiler-generated behavior.
The Rust compiler currently emits Entity lifecycle metadata only; it does not
bind or mutate lifecycle state, wrap Actions, or execute conformance. See the
[Lifecycle implementation boundary](../reference/domain/lifecycle.md#implementation-status).

Lifecycle Conformance checks the resulting state of an Action owner when an
application implements that flow.

```text
Owned State Change
  -> Lifecycle Conformance
  -> Invariant Validation
```

In the conceptual flow, an [Action](../reference/domain/action.md) triggers its
owner's [Lifecycle](../reference/domain/lifecycle.md), and the Lifecycle selects
the target state for that Action. The current compiler only records the
corresponding transition metadata.

An Action does not independently choose or change lifecycle state in that
conceptual model.

## Owner Scope

An [Aggregate](../reference/domain/aggregate.md) or
[Entity](../reference/domain/entity.md) with no Lifecycle skips this step.

A [Value Object](../reference/domain/value-object.md) and
[Domain Service](../reference/domain/domain-service.md) have no Lifecycle.

## Denied Result

When the resulting state does not conform to the allowed transition, the Action
is denied.

No state change or [Domain Event](../reference/domain/domain-event.md) occurs.

An internal denial returns to its parent. A public Action returns its owned
[Domain Error](../reference/domain/domain-error.md).
