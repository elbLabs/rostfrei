---
title: Aggregate
kind: reference
---

# Aggregate

Aggregate state may be projected through read-only [Queries](query.md). Queries receive the exact root by immutable reference and do not mutate, emit, deny, or access persistence.

## Definition

An **Aggregate** is a domain object that owns one consistency boundary.

It contains its entities and value objects and coordinates their behavior.

## Responsibility

An aggregate owns:

- its root state
- its entities and value objects
- its aggregate actions, attached decisions, attached invariant contracts, and
  lifecycle
- the consistency of changes within its boundary

An aggregate orchestrates behavior of its contained entities and value objects.
Its Actions may call any visible Decision in the same Bounded Context, including
a Decision attached to another owner. The compiler does not enforce Action call
permissions.

An aggregate can use its own Value Objects, Value Objects owned by its Entities,
and shared Value Objects owned by its Bounded Context. Aggregate-owned and
Entity-owned Value Objects remain local to their owner.

Child state changes occur only through the child’s internal action. The
Aggregate Action stages the completed aggregate candidate, explicitly validates
its complete invariant set, and commits it once.

An Aggregate creates and removes its contained Entities structurally. Creation
invokes Entity initialization; removal does not invoke a separate Entity delete
Action.

## Public Boundary

An aggregate is the only public entry point for behavior within its boundary.
The aggregate type is the public action-owner marker; its configured root is
the entity state passed to associated aggregate actions by convention.

Entities and value objects do not expose public actions. Aggregate actions invoke
their internal behavior when needed.

## Actions

Aggregate actions are declared as unrestricted public contract traits and
attached to the Aggregate through `actions = [TraitPath, ...]`:

```rust
mod contracts {
    use domain::domain_actions;

    #[domain_actions(aggregate(instance = TodoAggregateActions))]
    pub trait TodoActions {
        #[action(id = "rename", label = "Rename todo")]
        fn rename(root: &super::TodoRoot, input: String) -> super::TodoRenamed;
    }
}

#[derive(Aggregate)]
#[domain(
    id = "todo",
    label = "Todo",
    context = Planning,
    root = TodoRoot,
    actions = [contracts::TodoActions],
    events = [TodoRenamed],
)]
pub struct Todo;

impl contracts::TodoActions for Todo {
    fn rename(root: &TodoRoot, input: String) -> TodoRenamed {
        TodoRenamed {
            todo_id: root.id.clone(),
            title: input,
        }
    }
}
```

The trait must be `pub`, non-generic, and contain only action methods without
default bodies. An executable method begins with immutable `root: &RootType`,
followed by zero or one business `input`, and returns one Aggregate-owned Domain
Event. The Aggregate implements the attached contract once; the macro generates
the event-recording `AggregateInstance` adapter from that implementation.

Bring the generated instance trait into scope to call the Action through the
executable Aggregate:

```rust
use contracts::TodoAggregateActions as _;
aggregate.rename(title);
```

Command handlers map command data into one or more Action inputs and may call
multiple generated Action methods. Earlier successful calls update the
AggregateInstance state seen by later calls, while a final command rejection
causes the executor to discard the complete staged event set.

The Aggregate derive exposes attached descriptors through
`AggregateType::ACTION_CONTRACTS`. Registering the Aggregate in `domain_model!`
automatically projects them. Aggregate actions precede Entity, Value Object, and
Domain Service actions; within Aggregates, ordering follows `aggregates`
inventory order, `actions` attachment order, then trait method source order. An
omitted or empty `actions` list, an unattached implementation, or an Aggregate
omitted from `aggregates` does not project those actions. See [Action](action.md)
for shared contract, ordering, and trusted descriptor-extension rules.

## Decisions

Aggregate Decision contracts use `#[domain_decisions(aggregate)]`. The Aggregate
implements each contract attached through `decisions = [TraitPath, ...]`.
Attachment identifies ownership and projection; it does not limit the Decision
to Aggregate-owned Actions. See [Decision](decision.md).

## Invariants

Aggregate invariant contracts use `#[domain_invariants(aggregate)]`. The
Aggregate implements each contract attached through
`invariants = [TraitPath, ...]`; all attached traits form its one complete
invariant set. An Aggregate checker receives the configured root through
`<Self as InvariantOwnerType>::Candidate` and returns
`Option<InvariantViolation>`.

The Action explicitly calls
`<Aggregate as InvariantOwnerType>::validate_invariants(&candidate)` and
translates the complete violations into its Aggregate-owned Domain Error before
commit. Registering the Aggregate in `domain_model!` automatically projects its
attached invariants in attachment then trait method order. See
[Invariant](invariant.md).

## Model Shape

```yaml
id: TodoAggregate
label: Todo
root: TodoAggregate.Todo
```

## Boundaries

An aggregate does not:

- call a Decision from another Bounded Context
- invoke state-changing behavior outside its boundary
- change state owned by another aggregate
- coordinate multiple aggregates

An Aggregate may hold opaque IDs for Aggregates in the same
[Bounded Context](bounded-context.md). It does not access their state directly.

Cross-aggregate behavior belongs to a [Domain Service](domain-service.md).

## Related Concepts

- A [Bounded Context](bounded-context.md) contains aggregates.
- An [Entity](entity.md) belongs to an aggregate.
- A [Value Object](value-object.md) may belong to an aggregate, one of its
  entities, or its bounded context.
- A [Domain Service](domain-service.md) coordinates aggregates.
