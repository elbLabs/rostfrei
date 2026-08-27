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
calls an Aggregate- or Entity-owned Decision using ordinary inherent Rust
syntax.

## Visibility

An Action may call any Decision in the same
[Bounded Context](../reference/domain/bounded-context.md) when the Decision
function is visible at the call site. The Action and Decision do not need the
same owner.

Rust module and function visibility control whether the call compiles. The
domain compiler validates Decision declarations and owner attachment, but does
not enforce Action-to-Decision call permissions or infer a call graph from
method bodies.

## Input and Output

The Action supplies the Decision's ordinary typed parameters from facts already
available to it. The Decision returns `Result<T, E>`, where `Ok(T)` is the
accepted outcome and `Err(E)` is modeled business-denial data rather than a
Domain Error. The Action may continue with the success value or translate the
denial into an owner-appropriate
[Domain Error](../reference/domain/domain-error.md).

When the Action denies, no state changes or
[Domain Events](../reference/domain/domain-event.md) occur.

## Composition

An Action may call the Decisions required by its behavior. A Decision may also
compose pure rules in ordinary Rust, provided its complete contract remains one
typed parameter list and one `Result<T, E>` output.

The model records each Decision independently. It does not record gates,
derivations, Action-to-Decision links, call order, or a Decision dependency
graph.

## Implementation Status

Actions are the supported modeled Decision consumer in Rust Decisions v1.
Decision integration with Invariants and Lifecycles is not part of v1.

Decision evaluation is implemented in Rust. DMN and DMN dependency graphs are a
future implementation target, not a v1 runtime or metadata requirement.
