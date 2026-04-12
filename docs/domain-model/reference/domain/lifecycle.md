---
title: Lifecycle
kind: reference
---

# Lifecycle

## Definition

A **Lifecycle** defines an owner's allowed states and the Actions that may move
it between those states. The broader domain model permits lifecycle concepts for
[Aggregates](aggregate.md) and [Entities](entity.md), but current Rust compiler
support is Entity-only.

## Implementation Status

The implemented Entity lifecycle feature is metadata-only. The compiler derives,
validates, attaches, and projects lifecycle descriptors. It does not generate a
runtime state machine or execute the conceptual admission and conformance flows
described in the handbook.

In particular, the compiler generates no:

- binding to a current-state field
- initial-state assignment or Entity initialization
- Action wrapper or lifecycle admission execution
- lifecycle-state mutation, transaction, or rollback behavior
- lifecycle conformance check
- lifecycle denial or error translation
- event emission
- persistence or reconstitution behavior

Applications may implement those runtime concerns explicitly, but they must not
assume that attaching lifecycle metadata changes Action execution.

## Rust Representation

Declare lifecycle metadata as a separate, non-generic, fieldless enum. The
lifecycle requires a stable `id`, human-readable `label`, Entity `owner`, and
`initial` state. Every state requires its own explicit stable `id` and `label`.

```rust
use rostfrei_domain::{Entity, EntityLifecycle, domain_actions};

#[domain_actions(entity)]
trait TodoActions {
    #[action(id = "activate", label = "Activate todo")]
    fn activate(&mut self);

    #[action(id = "complete", label = "Complete todo")]
    fn complete(&mut self);
}

#[derive(EntityLifecycle)]
#[domain(
    id = "workflow",
    label = "Todo workflow",
    owner = Todo,
    initial = Draft,
)]
enum TodoLifecycle {
    #[domain(id = "draft", label = "Draft")]
    #[transition(action = TodoActions::ACTIVATE, to = Active)]
    Draft,
    #[domain(id = "active", label = "Active")]
    #[transition(action = TodoActions::COMPLETE, to = Completed)]
    Active,
    #[domain(id = "completed", label = "Completed")]
    Completed,
}

#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = TodoAggregate,
    actions = [TodoActions],
    lifecycle = TodoLifecycle,
)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
}
```

A transition is attached to its source-state variant with
`#[transition(action = TraitPath::REFERENCE, to = TargetVariant)]`. The typed
uppercase reference is generated from the Action's stable `#[action(id = ...)]`
ID; for example, `id = "activate"` produces `TodoActions::ACTIVATE`. Module-
qualified trait paths are supported.

Attach the lifecycle to its Entity with `lifecycle = Type`. Attachment is
optional, and an Entity can attach at most one lifecycle. The lifecycle's
`owner` must be that same Entity.

## Transition Semantics

The descriptor is a deterministic table keyed by `(source state, Action)`:

- each key has at most one target
- an omitted key means the Action is denied in that source state
- a self-transition is admitted and leaves lifecycle state unchanged
- a state with no outgoing transitions is a valid terminal state
- transitions have no guards and cannot reference [Decisions](decision.md)

These are the semantics represented by the metadata. The current compiler does
not read current state, perform the lookup, deny the call, or apply the target
state at runtime.

A transition Action must be a normally attached Action of the same Entity. An
Action implemented by the Entity but not attached is ineligible. An Action
available only through a descriptor extension is also ineligible, even if its
stable ID names that Entity.

## Projection

Registering an Entity through its normal model inventory projects the lifecycle
as a nested `lifecycle` member of that Entity. There is no separate top-level
lifecycle inventory. The nested member contains lifecycle `id`, `label`, states
in declaration order, the initial state ID, and transitions in source declaration
order. Transition Actions are projected as complete stable Action IDs.

An Entity with no attached lifecycle omits the `lifecycle` member rather than
projecting `null` or an empty descriptor.

```json
{
  "id": "workflow",
  "label": "Todo workflow",
  "states": [
    { "id": "draft", "label": "Draft" },
    { "id": "active", "label": "Active" },
    { "id": "completed", "label": "Completed" }
  ],
  "initial": "draft",
  "transitions": [
    {
      "source": "draft",
      "action": {
        "owner": {
          "kind": "entity",
          "id": {
            "aggregate": { "context": "todos", "local": "todo-list" },
            "local": "todo"
          }
        },
        "local": "activate"
      },
      "target": "active"
    }
  ]
}
```

## Invariant and Action Execution Boundary

[Invariants](invariant.md) are not lifecycle execution. An Action explicitly
stages its completed candidate and calls the owner's canonical invariant
validator. The compiler does not make lifecycle admission or conformance a
precondition for that call, select a transition while validating, or mutate
lifecycle state after validation.

Likewise, lifecycle metadata does not invoke an Action or wrap its ordinary Rust
trait method. If an application implements lifecycle execution, its Action flow
must explicitly perform any current-state lookup, admission denial, state
change, invariant validation, error translation, event handling, and commit.

## Ownership Boundaries

A lifecycle does not describe another owner's Actions or state. Value Objects
have no lifecycle because they have no independent state, and Domain Services
have no lifecycle because they own no state.

## Related Concepts

- An [Action](action.md) supplies the stable typed reference used by a transition.
- An [Invariant](invariant.md) explicitly validates a staged candidate; it does
  not execute lifecycle metadata.
- [Decision](decision.md) integration is not part of the lifecycle descriptor.
- A runtime implementation may translate lifecycle denial to an Action-owned
  [Domain Error](domain-error.md); the compiler does not generate that mapping.
