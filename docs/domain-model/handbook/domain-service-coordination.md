---
title: Domain Service Coordination
kind: handbook
---

# Domain Service Coordination

A [Domain Service](../reference/domain/domain-service.md) coordinates public
[Aggregate](../reference/domain/aggregate.md) Actions in one
[Bounded Context](../reference/domain/bounded-context.md).

```text
Public Domain Service Action
  -> Visible Action Decisions
  -> Aggregate interactions
  -> one state-changing Aggregate Action
  -> Service Action Outcome
```

A Domain Service coordinates at least two distinct Aggregate interactions in
one Bounded Context.

A Domain Service:

- may call any visible [Decision](../reference/domain/decision.md) in the same
  Bounded Context
- relies on ordinary Rust visibility; compiler Decision call permissions are not
  enforced
- invokes only public Aggregate Actions
- does not invoke [Entities](../reference/domain/entity.md) or
  [Value Objects](../reference/domain/value-object.md)
- does not change state directly
- does not own a Lifecycle, invariant contracts, or Domain Events

Each delegated Aggregate Action performs its complete Aggregate domain flow.

One Domain Service Action changes the state of at most one Aggregate. Other
Aggregate interactions are read-only.

Read-only Aggregate interactions complete before the one state-changing
Aggregate Action. That Action may produce
[Domain Events](../reference/domain/domain-event.md).

When an Aggregate interaction denies, the Domain Service translates that denial
into a Service-owned [Domain Error](../reference/domain/domain-error.md).

Follow-up Aggregate state changes occur through Domain Events and separate
public Actions.
