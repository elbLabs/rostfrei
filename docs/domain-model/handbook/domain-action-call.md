---
title: Domain Action Call
kind: handbook
---

# Domain Action Call

A Domain Action Call starts domain behavior.

A caller invokes one public [Action](../reference/domain/action.md) with
business input.

```text
Caller
  -> Aggregate Action
  -> Domain Service Action
```

Only [Aggregate](../reference/domain/aggregate.md) and
[Domain Service](../reference/domain/domain-service.md) Actions are public.

[Entity](../reference/domain/entity.md) and
[Value Object](../reference/domain/value-object.md) Actions are internal. They
are invoked only by their owning Aggregate or Entity.

## Action Input

Action input contains business data for the requested behavior.

An Action accepts zero or one business input parameter named `input`. Multiple
business values are grouped into one type. An Aggregate Action additionally
receives its owned state first as `root: &mut RootType`; that root is not
business input. Entity Actions use `&self` or `&mut self`, while Value Object
transformations consume `self`.

It does not define transport, aggregate identity, repository access,
persistence, or deployment.

## Owner Boundary

An Aggregate Action starts behavior within one Aggregate boundary.

A Domain Service Action starts coordination across public Aggregate Actions in
one [Bounded Context](../reference/domain/bounded-context.md).
