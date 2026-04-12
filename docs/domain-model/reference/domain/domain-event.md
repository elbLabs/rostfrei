---
title: Domain Event
kind: reference
---

# Domain Event

## Definition

A **Domain Event** records an allowed domain occurrence.

It describes a fact that happened, not a command or request to perform work.

## Ownership

An event belongs to exactly one [Aggregate](aggregate.md).

An aggregate action returns only events owned by that exact Aggregate, after its
behavior succeeds and all required invariants hold. It cannot return an event
owned by another Aggregate, including one in the same Context.

Internal [Entity](entity.md) and [Value Object](value-object.md) actions do not
emit events directly. Their aggregate emits the resulting domain event.

A [Domain Service](domain-service.md) propagates events from the Aggregate
actions it coordinates when the event-owning Aggregate and service have exactly
the same Context. It cannot propagate cross-Context events. It does not own or
emit domain events.

## Behavior

An event contains the business facts needed to describe the occurrence.

```yaml
id: TodoAggregate.TodoRenamed
label: Todo renamed
```

An event is immutable. A denied action emits no event.

`DomainEvent` supports non-generic structs. Its descriptor projects canonical
scalar, [Custom scalar](custom-scalar.md), identity, Value Object, and
aggregate-reference fields, including nested `Vec` and `Option` wrappers.
Contained Entity fields are invalid.

## Boundaries

An event does not:

- change state
- evaluate rules
- represent a requested operation
- represent a technical failure
- represent service orchestration

## Related Concepts

- An [Action](action.md) may emit an event after it succeeds.
- An [Aggregate](aggregate.md) owns events for changes within its boundary.
- A [Domain Error](domain-error.md) prevents an event when behavior is denied.
