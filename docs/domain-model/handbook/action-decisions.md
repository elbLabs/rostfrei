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

Rust Decisions v1 does not attach Decisions to Actions. An Action explicitly
calls a Decision through its owner-attached trait using ordinary Rust syntax.

## Visibility

An Action may call any Decision in the same
[Bounded Context](../reference/domain/bounded-context.md) when the Decision
contract is visible at the call site. The Action and Decision do not need the
same owner.

Rust module and trait visibility control whether the call compiles. The domain
compiler validates Decision declarations and owner attachment, but does not
enforce Action-to-Decision call permissions or infer a call graph from method
bodies.

## Input and Output

The Action constructs the Decision's one Value Object `input` from facts already
available to the Action. The Decision returns its direct Value Object output.

The output is data, not a `Result` or Domain Error. It may model an allowed or
denied business outcome and the facts explaining that outcome. The Action gives
that data operational meaning: it may continue, derive a value for later
behavior, or return an owner-appropriate
[Domain Error](../reference/domain/domain-error.md).

When the Action denies, no state changes or
[Domain Events](../reference/domain/domain-event.md) occur.

## Composition

An Action may call the Decisions required by its behavior. A Decision may also
compose pure rules in ordinary Rust, provided its complete contract remains one
Value Object input and one direct Value Object output.

The model records each Decision independently. It does not record gates,
derivations, Action-to-Decision links, call order, or a Decision dependency
graph.

## Implementation Status

Actions are the supported modeled Decision consumer in Rust Decisions v1.
Decision integration with Invariants and Lifecycles is not part of v1.

Decision evaluation is implemented in Rust. DMN and DMN dependency graphs are a
future implementation target, not a v1 runtime or metadata requirement.
