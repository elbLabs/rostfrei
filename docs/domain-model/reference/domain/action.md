---
title: Action
kind: reference
---

# Action

## Definition

An **Action** is a named domain operation.

It receives input, evaluates its domain behavior, and produces an allowed result
or a modeled denial.

## Ownership

An action belongs to exactly one owner:

- an [Aggregate](aggregate.md)
- an [Entity](entity.md)
- a [Value Object](value-object.md)
- a [Domain Service](domain-service.md)

## Visibility

Aggregate and Domain Service actions are public. Their action contracts are
unrestricted `pub` traits. Entity and Value Object actions are internal, so
their contract traits must have inherited or restricted visibility such as
`pub(crate)`, `pub(super)`, or `pub(in path)`; unrestricted `pub` is rejected.

Visibility is not stored in the descriptor.

## Rust Representation

Aggregate, Entity, Value Object, and Domain Service actions use contract traits
attached to their owner:

```rust
mod contracts {
    use domain::domain_actions;

    #[domain_actions(aggregate)]
    pub trait MailboxActions {
        #[action(id = "rename", label = "Rename mailbox")]
        fn rename(root: &mut super::MailboxRoot, input: String);
    }
}

#[derive(Aggregate)]
#[domain(
    id = "mailbox",
    label = "Mailbox",
    context = Mail,
    root = MailboxRoot,
    actions = [contracts::MailboxActions],
)]
pub struct Mailbox;

impl contracts::MailboxActions for Mailbox {
    fn rename(root: &mut MailboxRoot, input: String) {
        root.name = input;
    }
}
```

Every contract must name its owner kind explicitly: use
`#[domain_actions(aggregate)]` for a public Aggregate contract,
`#[domain_actions(entity)]` for an internal Entity contract,
`#[domain_actions(value_object)]` for an internal Value Object contract, or
`#[domain_actions(domain_service)]` for a public Domain Service contract. Bare
`#[domain_actions]` is not an action contract declaration.

A contract trait is non-generic, has no existing supertraits, and contains at
least one method. Every method has exactly one
`#[action(id = "...", label = "...")]` and no default body; other trait items
are rejected. Aggregate methods are associated functions beginning with
`root: &mut RootType`. Entity methods begin with `&self` or `&mut self`. Value
Object methods are either associated constructors with exactly one `input` or
transformations taking `self` by value and zero or one `input`. Domain Service
methods are associated functions with no receiver and zero or one `input`.

Each `actions = [TraitPath, ...]` entry on an Aggregate, Entity, Value Object, or
Domain Service attaches one contract. Paths cannot contain generic arguments,
use qualified-self syntax, or be repeated. The attached contract kind must match
the owner, and the owner must implement every attached trait. Implementing a
trait does not attach it.

Calls follow ordinary Rust trait rules. Fully qualified syntax works without an
import:

```rust
<Mailbox as contracts::MailboxActions>::rename(&mut root, name);
```

For owner-associated or method-call syntax, bring the trait into scope:

```rust
use contracts::MailboxActions as _;
Mailbox::rename(&mut root, name);
```

Entity contracts similarly use method-call syntax once their trait is in scope.
For a Value Object contract, importing the trait enables owner-associated
constructor calls such as `Money::from_minor(input)` and method calls such as
`money.normalize()`. Importing a Domain Service contract enables an
owner-associated call such as `TransferService::transfer(input)`. Fully
qualified syntax, such as
`<TransferService as TransferActions>::transfer(input)`, works without an
import.

The owner derive exposes attached descriptor slices through its
`ACTION_CONTRACTS` associated constant. Registering that owner in `domain_model!`
automatically projects those attached contracts. Omitting `actions`, using
`actions = []`, leaving a trait unattached, or omitting its owner from the
relevant model inventory omits those actions from automatic projection.

Automatic actions have deterministic owner-kind order:

1. Aggregate actions, by `aggregates` inventory order, attachment order, then
   trait method source order.
2. Entity actions, by `entities` inventory order, attachment order, then trait
   method source order.
3. Value Object actions, by `value_objects` inventory order, attachment order,
   then trait method source order.
4. Domain Service actions, by `services` inventory order, attachment order,
   then trait method source order.

Every descriptor has an `ActionId` composed from its owner's `ActionOwnerId` and
a lowercase kebab-case local ID. Duplicate IDs within one contract are rejected
by `domain_actions`; model registration enforces uniqueness across all attached
contracts and extensions.

### Trusted descriptor extensions

Generated code or descriptor adapters may provide additional trusted metadata by
implementing `domain::extension::ActionGroupType` and listing the type
in the optional `domain_model!` `action_extensions` inventory. Extensions append
in inventory order, preserving descriptor order, after all automatically
attached actions.

An extension supplies descriptors only; it does not declare methods or add
ordinary executable behavior. Use an explicitly typed `#[domain_actions(...)]`
contract trait for normal domain behavior.

Trusted extension authors are responsible for ensuring local IDs and labels have
valid metadata shape. During registration, the compiler enforces that the
extension's owner is registered, its descriptor slice is non-empty, every
descriptor owner exactly equals the declared owner's `ActionOwnerId`, and every
`ActionId` is unique across attached contracts and all extensions.

The Rust signature is the action contract. Action attributes contain only the
descriptor metadata `id` and `label`; inputs, successful outputs, and errors are
not repeated in attributes.

Custom public business inputs derive `DomainCommand`. Commands have stable
owner-scoped IDs, labels, fields, and an explicit `commands` model inventory.
The Rust input type links an action to a command; `#[action(command = ...)]` is
not supported. The command owner must exactly equal the Aggregate or Domain
Service action owner; a Domain Service contract therefore accepts only
service-owned commands.
Entity and Value Object actions cannot accept commands.
Canonical scalars and Value Objects remain valid inputs for existing contracts.
A [Custom scalar](custom-scalar.md) may be carried by an annotated derived
field, but is not yet supported as a raw Action input or output.

Each action accepts zero or one business input. When behavior needs multiple
values, group them into one input type. A typed parameter uses a simple
identifier pattern named `input`; aggregate state uses the separate `root`
parameter.

Supported signatures are:

- Aggregate contract trait: associated function whose first parameter is
  `root: &mut RootType`, followed by zero or one `input`. `RootType` must equal
  `<Owner as AggregateType>::Root`.
- Entity contract trait: method with an `&self` or `&mut self` receiver, followed
  by zero or one `input`. Explicit receiver lifetimes and typed receivers are
  unsupported.
- Value Object contract trait: associated constructor with exactly one `input`,
  or transformation taking `self` by value followed by zero or one `input`.
- Domain Service contract trait: unrestricted `pub` trait whose methods are
  associated functions with zero or one `input` and no receiver.

An infallible action returns its output directly; omitted return syntax is the
plain output `()`. A fallible action returns direct `Result<Output, Error>`,
`core::result::Result<Output, Error>`, or `std::result::Result<Output, Error>`.
Leading `::` is accepted. Other aliases and custom result types are plain
output. For a recognized `Result`, `Error` must implement
`DomainErrorType<Owner = action owner>`; a Domain Service action therefore uses
only an error owned by that exact service. A Value Object action's successful
output must resolve to that exact Value Object type: directly for an infallible
action or as the `Result` output for a fallible action. `Self` and an equivalent
type alias satisfy this semantic requirement. Scalars, unit, other Value
Objects, Domain Events, and wrapped outputs such as `Option<Self>` or `Vec<Self>`
do not. Its error, when present, must be owned by that exact Value Object.

Other successful outputs may be unit, a scalar, a Value Object, or any supported
`Option` and `Vec` nesting of those types. An Aggregate Action may also return a
Domain Event owned by that exact Aggregate. A Domain Service Action may return
an event owned by any Aggregate whose Context is exactly the service Context.
Entity Actions cannot return Domain Events. These event rules apply recursively
through every `Option` and `Vec` wrapper.

Derived output types implement these contracts automatically. A manual
`ActionOutputType<Contract>` implementation is a trusted extension of the
compiler contract and is responsible for preserving the same ownership rules.

## Decisions

An Action is the supported modeled consumer of [Decisions](decision.md) in Rust
Decisions v1. It may call any Decision in the same
[Bounded Context](bounded-context.md) when ordinary Rust visibility makes the
Decision contract accessible. The Action and Decision do not need the same
owner.

Decision calls are ordinary Rust trait calls. Action descriptors contain no
Decision references, and action attributes have no Decision metadata. The
compiler validates Decision contracts and attachment but does not inspect Action
bodies, infer a call graph, or enforce call permissions between same-context
owners.

A Decision returns a Value Object, including modeled business-denial data. The
Action interprets that output and may translate a denied outcome into an
owner-appropriate Domain Error.

## Denials

A public action returns only [Domain Errors](domain-error.md) owned by its
public owner.

An aggregate action translates internal entity or value-object denials into an
aggregate-owned domain error. A domain-service action translates aggregate
denials into a service-owned domain error.

## Delegation

An action invokes behavior only within the allowed ownership direction.

A parent may invoke an action of a directly owned child. It may not bypass an
intermediate owner.

```text
Domain Service action
  -> public Aggregate action

Aggregate action
  -> internal directly owned Entity or Value Object action

Entity action
  -> internal directly owned Value Object action
```

## Lifecycle

An action follows the lifecycle of its owner.

The owner's lifecycle determines whether the action is allowed in its current
state and whether the action changes that state.

An action does not evaluate or change another owner's lifecycle.

## Invariant Validation

After transition logic produces a completed candidate and before commit, an
Action explicitly calls its owner's canonical invariant validator. Checkers
return `InvariantViolation` data; the Action translates the complete collection
into its owner-owned Domain Error. The compiler injects no validation, Action
wrapper, or rollback behavior. See [Invariant](invariant.md).

## Model Shape

```rust
ActionDescriptor {
    id: ActionId {
        owner: ActionOwnerId::Aggregate(Todo::DESCRIPTOR.id),
        local: "assign-todo",
    },
    label: "Assign todo",
    input: Some(ActionInputDescriptor::DomainCommand(AssignTodo::DESCRIPTOR.id)),
    output: Some(ActionOutputDescriptor::DomainEvent(TodoAssigned::DESCRIPTOR.id)),
    error: Some(TodoAssignmentDenied::DESCRIPTOR.id),
}
```

Compiled JSON uses `input`, `output`, and `error` on every action, with `null`
for absent contracts. Commands are referenced as `{ "kind": "domainCommand",
"id": ... }`; their fields are projected once in top-level `domainCommands`.
Successful values preserve optional/list wrappers and reference scalar, Value
Object, or Domain Event contracts by kind. Errors are projected as
`DomainErrorId` references.

## Boundaries

An action does not:

- evaluate another owner's invariant
- call a Decision from another Bounded Context
- change state outside its owner's boundary
- bypass an allowed delegation boundary

## Related Concepts

- A [Decision](decision.md) provides pure rule evaluation within the same Bounded
  Context.
- An [Invariant](invariant.md) protects local state.
- A [Lifecycle](lifecycle.md) controls owner state transitions.
- A [Domain Error](domain-error.md) defines a modeled denial.
- A [Domain Event](domain-event.md) records an allowed domain occurrence.
