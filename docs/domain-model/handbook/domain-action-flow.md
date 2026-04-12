---
title: Domain Action Flow
kind: handbook
---

# Domain Action Flow

A Domain Action Flow composes the domain steps of one public
[Action](../reference/domain/action.md).

## Aggregate Action

```text
Domain Action Call
  -> Lifecycle Admission
  -> Action Decisions
  -> Owned Behavior
     -> Delegation
     -> Owned State Change
  -> Lifecycle Conformance
  -> Explicit Invariant Validation
  -> Domain Event Recording
  -> Action Outcome
```

Owned Behavior may invoke directly owned child Actions and change
Aggregate-owned state. The child Actions run their own complete local flows.

## Domain Service Action

```text
Domain Action Call
  -> Visible Action Decisions
  -> Domain Service Coordination
     -> Aggregate interactions
     -> one state-changing Aggregate Action
  -> Action Outcome
```

A [Domain Service](../reference/domain/domain-service.md) coordinates Aggregate
interactions across at least two Aggregates in one Bounded Context. It changes
the state of at most one Aggregate per Action.

## Denied Branch

A denial at any domain step enters
[Denial Translation](denial-translation.md).

```text
Lifecycle denial
Action gate denial
Child denial
Complete invariant violations
Delegated Aggregate denial
  -> Denial Translation
  -> Denied Action Outcome
```

The Action translates a complete invariant violation collection into its
owner-owned Domain Error. Validation and rollback are not injected into the
Action.

A denied Action changes no state and records no
[Domain Event](../reference/domain/domain-event.md).
