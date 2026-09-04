# ADR 0035: Executable entity lifecycles

## Status

Accepted.

## Context

The original `EntityLifecycle` contract described an ordered vocabulary of
states. It did not identify an initial state or define legal changes between
states. Applications therefore needed a second status enum for stored state
and scattered transition checks across actions. The lifecycle metadata could
document names, but it could not protect behavior.

Invariants answer whether a candidate state is valid. Decisions evaluate
business facts and produce an outcome. A lifecycle has a narrower temporal
responsibility: given the current state and a requested transition, determine
whether the next state is legal.

## Decision

An entity's actual stored state enum may derive `EntityLifecycle`. It declares
one explicit initial state and implements the `LifecycleState` trait:

```rust
#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "rental-status", label = "Bicycle rental status")]
#[lifecycle(initial = Available)]
pub enum BicycleStatus {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
```

A separate fieldless enum names the requested changes. It derives
`StateTransition` and declares one edge per variant:

```rust
#[derive(StateTransition)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[edge(id = "rent", label = "Rent", from = Available, to = Rented)]
    Rent,
    #[edge(id = "return", label = "Return", from = Rented, to = Available)]
    Return,
}
```

`LifecycleState::evaluate` is pure. It returns a `StateChange` for a legal edge
and `InvalidStateTransition` otherwise. The traits are the runtime contract;
the derives only generate their straightforward implementations and stable
descriptors. Applications may implement the traits manually.

Lifecycle evaluation owns transition topology only. Context-sensitive rules,
such as customer eligibility or an asset's condition, remain ordinary domain
decisions and invariants. Events remain facts, and infallible event application
does not become a fallible transition API.

The first version intentionally supports one active state, one initial state,
and one source/target edge per transition variant. It does not implement
guards, side effects, final or hierarchical states, parallel regions, history,
or SCXML execution semantics.

The typed domain structure stores the state declaration in `lifecycle.rs` and
the transition declaration in the sibling `transition.rs`. The lifecycle
directory remains anchored by `lifecycle.rs`; `transition.rs` is a companion,
not a second directory role.

## Consequences

Lifecycle declarations now require `#[lifecycle(initial = ...)]` and their
state enums must satisfy `Copy + Eq`. Code can discover the initial state,
resolve stable state and transition IDs, evaluate transitions, and inspect
transition descriptors.

This is a breaking source change for existing lifecycle declarations and
`EntityLifecycleDescriptor` struct literals because the descriptor now
includes `initial`. Metadata-only lifecycle enums should be replaced by the
entity's authoritative stored state where possible.

The lifecycle remains owner-independent and is not automatically projected
onto an entity in the compiled domain model. Whole-graph reachability analysis
and richer state-machine semantics can be added later if demonstrated domain
needs justify them.

This decision supersedes the lifecycle-specific statement in
[ADR 0021](0021-explicit-entity-definition-and-owner-independent-tags.md) that
initial states and transitions are intentionally unspecified. The remaining
owner-independent metadata decisions in ADR 0021 still apply.
