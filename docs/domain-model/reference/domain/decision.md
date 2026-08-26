---
title: Decision
kind: reference
---

# Decision

## Definition

A **Decision** is a named, pure, stateless domain rule implemented in Rust.

It receives one modeled [Value Object](value-object.md) and returns one modeled
Value Object. A Decision does not own state or obtain state implicitly. Its input
contains all facts required to evaluate the rule.

## Ownership

A Decision belongs to exactly one domain object:

- an [Aggregate](aggregate.md)
- an [Entity](entity.md)
- a [Value Object](value-object.md)
- a [Domain Service](domain-service.md)

Ownership identifies and attaches the Decision. It does not restrict the
Decision to Actions owned by that same object. Any [Action](action.md) in the
same [Bounded Context](bounded-context.md) may call a Decision when ordinary
Rust visibility makes its contract accessible.

The compiler validates the Decision contract and its attachment. It does not
enforce which Actions have permission to call a visible Decision.

## Rust Representation

Decision methods are declared on owner-kind contract traits:

```rust
mod contracts {
    use domain::domain_decisions;

    #[domain_decisions(aggregate)]
    pub trait TodoDecisions {
        #[decision(id = "can-assign", label = "Can assign todo")]
        fn can_assign(input: CanAssignInput) -> CanAssignOutcome;
    }
}

#[derive(Aggregate)]
#[domain(
    id = "todo",
    label = "Todo",
    context = Planning,
    root = TodoRoot,
    decisions = [contracts::TodoDecisions],
)]
pub struct Todo;

impl contracts::TodoDecisions for Todo {
    fn can_assign(input: CanAssignInput) -> CanAssignOutcome {
        todo!()
    }
}
```

Every contract names its owner kind explicitly:

- `#[domain_decisions(aggregate)]`
- `#[domain_decisions(entity)]`
- `#[domain_decisions(value_object)]`
- `#[domain_decisions(domain_service)]`

Every Decision method has exactly one
`#[decision(id = "...", label = "...")]`. The `id` is the owner-local stable
ID, and `label` is its human-readable name. The Rust signature is the complete
input and output contract; the attribute contains no type or consumer metadata.

Each method is an associated function with no receiver and no `root` parameter.
It has exactly one by-value parameter named `input`. That input is one owned
Value Object visible to the Decision owner. The return type is one direct Value
Object.

A Decision signature does not accept or return:

- `&self`, `&mut self`, or `self`
- an Aggregate root
- zero inputs or multiple inputs
- raw scalars
- [Domain Commands](domain-command.md)
- [Domain Events](domain-event.md)
- [Domain Errors](domain-error.md)
- `Result`
- unit
- a top-level `Option` or `Vec`

A Value Object used as the input or output may model scalar fields and supported
nested value shapes according to the Value Object contract. The Decision
boundary itself remains exactly one Value Object in and one Value Object out.

The owner implements every attached contract and attaches it through
`decisions = [TraitPath, ...]`. Implementing a trait does not attach it. An
omitted or empty `decisions` list does not project the contract's Decisions.

## Calls

Calls use ordinary Rust trait rules. Fully qualified syntax works without an
import:

```rust
let outcome =
    <Todo as contracts::TodoDecisions>::can_assign(input);
```

With the contract trait in scope, owner-associated syntax is available:

```rust
use contracts::TodoDecisions as _;
let outcome = Todo::can_assign(input);
```

Rust module and trait visibility determine whether an Action can make the call.
The compiled model does not contain Action-to-Decision links, and
`#[action(...)]` has no Decision metadata. The compiler does not infer calls
from function bodies or enforce call permissions between same-context owners.

## Business Denial

A Decision does not return a Domain Error. Business denial is ordinary modeled
data in the output Value Object. The output may represent outcomes such as
allowed and denied, including the business facts that explain that result.

An Action interprets the output. When its public contract requires a denial, the
Action may translate denied output data into its own owner-appropriate Domain
Error. That translation does not make the Decision fallible and does not add an
Action-to-Decision metadata relationship.

## Purity and Statelessness

A Decision is normatively pure and stateless. It does not mutate domain state,
load state or infrastructure data, invoke Actions, emit events, or depend on
hidden mutable state. Equal input represents the same domain facts and produces
the same output.

Rust Decisions v1 validates the declared signature and modeled types. It does
not mechanically inspect the method body for I/O, mutation through external
state, nondeterminism, or other side effects. Authors preserve these semantics.

## Consumers

Rust Decisions v1 supports Actions as the modeled consumer. Any visible Action
in the same Bounded Context may call a Decision through ordinary Rust.

Decision integration with [Invariants](invariant.md) and
[Lifecycles](lifecycle.md) is not part of v1. Their descriptors do not reference
Decisions, and this reference does not define an execution relationship for
them.

## Model Shape

The attached contract projects one descriptor per Decision:

```rust
DecisionDescriptor {
    id: DecisionId {
        owner: DecisionOwnerId::Aggregate(Todo::DESCRIPTOR.id),
        local: "can-assign",
    },
    label: "Can assign todo",
    input: DecisionInputDescriptor::ValueObject(CanAssignInput::DESCRIPTOR.id),
    output: DecisionOutputDescriptor::ValueObject(CanAssignOutcome::DESCRIPTOR.id),
    implementation: DecisionImplementationDescriptor::Rust,
}
```

The descriptor contains exactly five fields: `id`, `label`, `input`, `output`,
and `implementation`. The `DecisionId` combines the typed owner ID with the
owner-local Decision ID. `input` is a
`DecisionInputDescriptor::ValueObject(ValueObjectId)`, `output` is a
`DecisionOutputDescriptor::ValueObject(ValueObjectId)`, and `implementation` is
`DecisionImplementationDescriptor::Rust`. Value Object fields remain on the
Value Object inventory items.

Compiled model JSON uses a top-level `decisions` inventory:

```json
{
  "decisions": [
    {
      "id": {
        "owner": {
          "kind": "aggregate",
          "id": {
            "context": "planning",
            "local": "todo"
          }
        },
        "local": "can-assign"
      },
      "label": "Can assign todo",
      "input": {
        "kind": "valueObject",
        "id": {
          "owner": {
            "kind": "aggregate",
            "id": {
              "context": "planning",
              "local": "todo"
            }
          },
          "local": "can-assign-input"
        }
      },
      "output": {
        "kind": "valueObject",
        "id": {
          "owner": {
            "kind": "aggregate",
            "id": {
              "context": "planning",
              "local": "todo"
            }
          },
          "local": "can-assign-outcome"
        }
      },
      "implementation": {
        "kind": "rust"
      }
    }
  ]
}
```

Every Decision has non-null Value Object `input` and `output` descriptors and an
`implementation` descriptor whose v1 kind is `rust`. A Decision descriptor has
no consumer, Action, gate, command, event, or error field. There is no
implementation path or DMN field.

## Implementation Status

Rust contract traits and owner attachment are the Decisions v1 implementation.
DMN is a future implementation target. V1 does not load a DMN file, project a
DMN requirements graph, or require DMN metadata.

## Related Concepts

- An [Action](action.md) is the supported modeled consumer in v1.
- A [Value Object](value-object.md) defines the Decision input and output.
- A [Bounded Context](bounded-context.md) bounds Decision visibility and use.
