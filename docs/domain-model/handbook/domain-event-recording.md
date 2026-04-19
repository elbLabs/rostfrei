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
not record Domain Events.

An Aggregate Action explicitly records zero or more Domain Events with
`self.raise(event)`.

Domain Events are recorded only after Lifecycle Conformance and Invariant
Validation succeed.

A fallible Action completes every denial check before its first `raise` and
records no event when denied. Returning an error after raising violates the
Action contract; there is no independent Action rollback boundary.

A successful Aggregate state change and its Domain Events are one atomic domain
outcome.
