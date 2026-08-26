---
title: Domain Event Recording
kind: handbook
---

# Domain Event Recording

Domain Event Recording records the business facts produced by a successful
[Aggregate Action](../reference/domain/action.md).

```text
Owned State Change
  -> Lifecycle Conformance
  -> Invariant Validation
  -> Domain Event Recording
  -> Allowed Result
```

Only an [Aggregate](../reference/domain/aggregate.md) Action records
[Domain Events](../reference/domain/domain-event.md), and it records only events
owned by that exact Aggregate.

[Entity](../reference/domain/entity.md) and
[Value Object](../reference/domain/value-object.md) Actions do not record
Domain Events. A [Domain Service](../reference/domain/domain-service.md) does
not record Domain Events; it returns the Aggregate Domain Events produced by the
Aggregate Actions it coordinates when those Aggregates have exactly the
service's Bounded Context.

An Aggregate Action may record zero or more Domain Events.

Domain Events are recorded only after Lifecycle Conformance and Invariant
Validation succeed.

A denied Action records no Domain Event.

A successful Aggregate state change and its Domain Events are one atomic domain
outcome.
