---
title: Action Decisions
kind: handbook
---

# Action Decisions

An [Action](../reference/domain/action.md) may call one or more
[Decisions](../reference/domain/decision.md) as part of its domain behavior.

```text
Domain Action Call
  -> Lifecycle Admission
  -> Action Decisions
  -> Owned Behavior
```

Decisions are stateless policies owned by an Aggregate or Entity and organized
in explicitly attached groups. They are not attached to Actions. An Action calls
a visible Decision using ordinary inherent Rust syntax.

## Visibility

An Action may call any Decision in the same
[Bounded Context](../reference/domain/bounded-context.md) when the Decision
function is visible at the call site. The Action and Decision do not need the
same owner.

The user-declared Decision group marker and the Decision function use normal
Rust visibility. `domain_decisions` has no visibility option. Group visibility
allows the marker to be named where it is attached; function visibility controls
where the policy can be called.

The domain compiler validates Decision declarations and exact owner attachment.
It does not enforce Action-to-Decision call permissions or infer a call graph
from method bodies.

## Inputs

The Action supplies the Decision's ordinary typed parameters from facts already
available to it. A Decision may take a supported scalar or Value Object by value
or by top-level immutable reference. Owned `T` and borrowed `&T` produce
identical model metadata, so the Action can borrow without changing the compiled
policy contract.

Mutable borrows and nested references are rejected. Decisions do not receive an
owner root or `self`; the Action must pass every relevant fact explicitly.

## Matching Outcomes

A Decision returns a non-generic enum deriving `DecisionOutcome`, not `Result`.
Every variant is a stable, Decision-scoped outcome with unit, tuple, or named
struct shape. The Decision does not classify variants as accepted or denied.

The Action exhaustively matches the enum and translates each relevant variant
into its own behavior:

```rust
match Todo::can_assign(&assignee, open_assignment_count) {
    AssignmentOutcome::Assignable { remaining_capacity } => {
        self.raise(TodoAssigned {
            assignee,
            remaining_capacity,
        });
        Ok(())
    }
    AssignmentOutcome::RequiresReview(reason) => {
        self.raise(AssignmentReviewRequested { assignee, reason });
        Ok(())
    }
    AssignmentOutcome::Unavailable => {
        Err(AssignmentDenied::capacity_reached())
    }
}
```

Here two outcomes become allowed Action branches with different Domain Events,
while one becomes an owner-appropriate
[Domain Error](../reference/domain/domain-error.md). That is this Action's
translation, not global outcome metadata. Another Action may translate the same
policy outcome differently.

When an executable Aggregate Action returns an error, it must perform denial
checks before its first raise. A caught Action error has no independent rollback
boundary; see [Action Outcome](action-outcome.md).

## Composition

An Action may call the Decisions required by its behavior. A Decision may also
compose pure Rust rules, provided its complete contract remains an explicit
parameter list and one closed `DecisionOutcome` enum.

The model records each Decision and its ordered outcomes independently. It does
not record Decision groups, gates, derivations, Action-to-Decision links, call
order, match translations, or a Decision dependency graph.

## Implementation Status

Actions are the supported modeled Decision consumer. Decision integration with
Invariants and Lifecycles is not defined.

Decision evaluation is implemented in Rust. DMN and DMN dependency graphs are a
future implementation target, not a current runtime or metadata requirement.
