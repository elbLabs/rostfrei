---
title: Entity
kind: reference
---

# Entity

## Definition

An **Entity** is a domain object with stable identity and mutable state.

It belongs to one [Aggregate](aggregate.md).

## Responsibility

An entity owns:

- its state
- its internal actions, attached decisions, attached invariant contracts, and
  lifecycle
- its contained value objects

An Entity Action may call any visible Decision in the same Bounded Context. The
compiler does not enforce Action call permissions. Attached invariant
contracts remain local to the Entity. An Entity may invoke only its own
[Value Objects](value-object.md).

## Public Boundary

Entity actions are internal.

Its [Aggregate](aggregate.md) invokes entity behavior as part of an aggregate
action. An entity does not invoke another entity, aggregate, or domain service.

## Model Shape

```rust
#[derive(Entity)]
#[domain(
    id = "todo",
    label = "Todo",
    owner = TodoAggregate,
    actions = [TodoActions, scheduling::TodoSchedulingActions],
)]
struct Todo {
    #[domain(identity)]
    id: TodoId,
    title: String,
    #[domain(value_object)]
    schedule: Option<Vec<Schedule>>,
    #[domain(entity)]
    steps: Vec<Step>,
    #[domain(aggregate_ref = ProjectAggregate)]
    project_id: Option<ProjectId>,
}
```

The identity type declares this exact Entity as its owner:

```rust
#[derive(DomainIdentity)]
#[domain(owner = Todo)]
struct TodoId(String);
```

The descriptor records every field in source order. Raw identifiers are
normalized, so `r#type` is named `type`. The identity field is included as an
`identity` value and remains separately identified by the identity descriptor.
Both project the stable typed [Domain Identity](domain-identity.md) ID.

Untagged fields support `bool`, `String`, `char`, every fixed-width signed and
unsigned integer, `isize`, `usize`, `f32`, and `f64`. A field may explicitly use
a [Custom scalar](custom-scalar.md) through
`#[domain(scalar = Provider)]`. Other domain fields use one explicit role:
`identity`, `entity`, `value_object`, or `aggregate_ref = AggregateType`.
Identity cannot be wrapped. `Vec` and `Option` may nest to any depth and retain
outermost-to-innermost shape. The model records semantic types rather than Rust
type strings.

Only canonical `Vec`, `Option`, and `String` paths are recognized. Type aliases,
custom containers, maps, sets, references, arrays, slices, tuples, generic base
types, and untagged custom types are unsupported.

## Decisions

Entity Decision contracts use `#[domain_decisions(entity)]`. The Entity
implements each contract attached through `decisions = [TraitPath, ...]`.
Attachment identifies ownership and projection; it does not limit the Decision
to Entity-owned Actions. See [Decision](decision.md).

## Invariants

Entity invariant contracts use `#[domain_invariants(entity)]`. The Entity
implements each contract attached through `invariants = [TraitPath, ...]`; all
attached traits form its one complete invariant set. Checkers receive
`&<Self as InvariantOwnerType>::Candidate`, which is `&Self`, and return
`Option<InvariantViolation>`.

The Entity Action explicitly calls the canonical owner validator and translates
the complete violations into its Entity-owned Domain Error before commit.
Registering the Entity in `domain_model!` automatically projects attached
invariants in attachment then trait method order. See [Invariant](invariant.md).

## Actions

Entity actions are declared as internal contract traits and attached through
`actions = [TraitPath, ...]`:

```rust
#[domain_actions(entity)]
trait TodoActions {
    #[action(id = "rename", label = "Rename todo")]
    fn rename(&mut self, input: String) -> Result<(), TodoDenied>;
}

impl TodoActions for Todo {
    fn rename(&mut self, input: String) -> Result<(), TodoDenied> {
        self.title = input;
        Ok(())
    }
}
```

An Entity contract requires explicit `#[domain_actions(entity)]`; bare
`#[domain_actions]` is not an action contract declaration. The trait cannot be
unrestricted `pub` or generic. It contains only methods, every method requires
exactly one `#[action]`, and methods have no default bodies. Each method takes
`&self` or `&mut self` and zero or one business `input`. The Entity derive
verifies at compile time that the Entity implements every attached trait.

Calls use ordinary Rust trait semantics. Bring the contract trait into scope to
use method syntax, commonly with `use TodoActions as _;`, then call
`todo.rename(input)`. Fully qualified syntax such as
`<Todo as TodoActions>::rename(&mut todo, input)` works without importing the
trait.

The Entity derive exposes attached descriptors through
`EntityType::ACTION_CONTRACTS`. Registering the Entity in `domain_model!`
automatically projects them after Aggregate actions and before Value Object and
Domain Service actions. Across Entities, model `entities` inventory order comes
first, followed by `actions` attachment order and trait method source order.
Omitting `actions` is equivalent to `actions = []`. An implemented but
unattached trait is not projected, and an Entity omitted from the model does not
project actions. See [Action](action.md) for trusted descriptor-extension rules.

## Boundaries

An entity does not:

- call a Decision from another Bounded Context
- change another entity's state directly
- coordinate entities or aggregates
- expose behavior outside its aggregate

An Entity may hold opaque IDs for Aggregates in the same
[Bounded Context](bounded-context.md). It does not access their state directly.
An Entity may structurally contain Value Objects declared at entity, aggregate,
or bounded-context scope. Visibility and scope enforcement are deferred.

## Related Concepts

- An [Aggregate](aggregate.md) contains entities.
- A [Value Object](value-object.md) supplies owned value behavior.
- An [Action](action.md) may be owned by an entity but is internal.
