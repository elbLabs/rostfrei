---
title: Decision
kind: reference
---

# Decision

## Definition

A **Decision** is a named, pure, stateless domain rule implemented as an
associated function on an Aggregate or Entity. Its parameters contain all facts
required to evaluate the rule, and its `Result<T, E>` expresses the accepted or
denied outcome.

## Ownership

A Decision belongs to exactly one domain object:

- an [Aggregate](aggregate.md)
- an [Entity](entity.md)

Value Objects and Domain Services cannot own Decisions. Ownership identifies
and projects the Decision, but does not restrict it to Actions owned by the same
object. Any [Action](action.md) in the same
[Bounded Context](bounded-context.md) may call a Decision when ordinary Rust
visibility makes its function accessible.

The compiler validates the Decision declaration and its attachment. It does not
enforce which Actions have permission to call a visible Decision.

## Rust Representation

Decision methods are declared directly in one inherent owner `impl` block:

```rust
#[derive(Aggregate)]
#[domain(
    id = "todo",
    label = "Todo",
    context = Planning,
    root = TodoRoot,
    decisions,
)]
pub struct Todo;

#[domain_decisions(aggregate)]
impl Todo {
    #[decision(id = "can-assign", label = "Can assign todo")]
    fn can_assign(
        assignee: Assignee,
        open_assignment_count: u32,
    ) -> Result<AssignmentApproval, AssignmentDenial> {
        todo!()
    }
}
```

The derive attribute uses the marker `decisions` to attach the inherent Decision
block to the owner. The impl attribute names its owner kind explicitly:

- `#[domain_decisions(aggregate)]`
- `#[domain_decisions(entity)]`

Every Decision method has exactly one
`#[decision(id = "...", label = "...")]`. The `id` is the owner-local stable
ID, and `label` is its human-readable name. The Rust signature supplies all
parameter, success, and denial metadata.

Each method is an associated function with no receiver or Aggregate root
parameter. It may have zero or more by-value parameters. Every parameter uses a
simple immutable identifier and a supported scalar or
[Value Object](value-object.md) type. Every Decision returns `Result<T, E>`;
supported result types are scalars, Value Objects, and unit. Unit is projected
as no output or error descriptor.

A Decision signature does not accept or return:

- `&self`, `&mut self`, or `self`
- an Aggregate root
- borrowed parameters
- [Domain Commands](domain-command.md)
- [Domain Events](domain-event.md)
- [Domain Errors](domain-error.md)
- a top-level `Option` or `Vec`
- async, unsafe, extern, variadic, or generic functions

A Value Object used at the boundary may model supported nested value shapes
according to the Value Object contract. The Decision boundary itself remains
flat: each function parameter becomes one named parameter descriptor.

The owner has at most one `#[domain_decisions(...)]` impl. Omitting the
`decisions` marker from the owner derive leaves the functions callable but does
not project their descriptors. A marker without a matching Decision impl fails
to compile.

## Calls

Calls use ordinary inherent Rust syntax and preserve the authored `Result` type:

```rust
let approval = Todo::can_assign(assignee, open_assignment_count)?;
```

Rust module and method visibility determine whether an Action can make the call.
The compiled model does not contain Action-to-Decision links, and
`#[action(...)]` has no Decision metadata. The compiler does not infer calls
from function bodies or enforce call permissions between same-context owners.

## Business Denial

A Decision expresses business denial as the `Err(E)` branch of its result. The
error type is modeled Decision output data, not a Domain Error. An Action may
translate it into an owner-appropriate Domain Error when its public contract
requires one. That translation does not add an Action-to-Decision metadata
relationship.

## Purity and Statelessness

A Decision is normatively pure and stateless. It does not mutate domain state,
load state or infrastructure data, invoke Actions, emit events, or depend on
hidden mutable state. Equal parameters represent the same domain facts and
produce the same result.

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

The attached inherent block projects one descriptor per Decision:

```rust
DecisionDescriptor {
    id: DecisionId {
        owner: DecisionOwnerId::Aggregate(Todo::DESCRIPTOR.id),
        local: "can-assign",
    },
    label: "Can assign todo",
    parameters: &[
        DecisionParameterDescriptor {
            name: "assignee",
            input: DecisionInputDescriptor::ValueObject(Assignee::DESCRIPTOR.id),
        },
        DecisionParameterDescriptor {
            name: "open_assignment_count",
            input: DecisionInputDescriptor::Scalar(ScalarType::U32),
        },
    ],
    output: Some(DecisionOutputDescriptor::ValueObject(
        AssignmentApproval::DESCRIPTOR.id,
    )),
    error: Some(DecisionOutputDescriptor::ValueObject(
        AssignmentDenial::DESCRIPTOR.id,
    )),
    implementation: DecisionImplementationDescriptor::Rust,
}
```

The descriptor contains `id`, `label`, `parameters`, `output`, `error`, and
`implementation`. `DecisionId` combines the typed owner ID with the owner-local
Decision ID. Parameters retain their Rust names and source order. Value Object
fields remain on the Value Object inventory items.

Compiled model JSON uses a top-level `decisions` inventory:

```json
{
  "decisions": [
    {
      "id": {
        "owner": {
          "kind": "aggregate",
          "id": { "context": "planning", "local": "todo" }
        },
        "local": "can-assign"
      },
      "label": "Can assign todo",
      "parameters": [
        {
          "name": "assignee",
          "input": {
            "kind": "valueObject",
            "id": {
              "owner": {
                "kind": "aggregate",
                "id": { "context": "planning", "local": "todo" }
              },
              "local": "assignee"
            }
          }
        },
        {
          "name": "open_assignment_count",
          "input": { "kind": "scalar", "scalar": "u32" }
        }
      ],
      "output": {
        "kind": "valueObject",
        "id": {
          "owner": {
            "kind": "aggregate",
            "id": { "context": "planning", "local": "todo" }
          },
          "local": "assignment-approval"
        }
      },
      "error": {
        "kind": "valueObject",
        "id": {
          "owner": {
            "kind": "aggregate",
            "id": { "context": "planning", "local": "todo" }
          },
          "local": "assignment-denial"
        }
      },
      "implementation": { "kind": "rust" }
    }
  ]
}
```

`parameters` is always an array and may be empty. Unit success or denial types
project `output` or `error` as `null`. `implementation.kind` is `rust` in v1. A
Decision descriptor has no consumer, Action, gate, command, event,
implementation path, or DMN field.

## Implementation Status

Rust inherent owner functions and marker attachment are the Decisions v1
implementation. DMN is a future implementation target. V1 does not load a DMN
file, project a DMN requirements graph, or require DMN metadata.

## Related Concepts

- An [Action](action.md) is the supported modeled consumer in v1.
- A [Value Object](value-object.md) can define a Decision parameter, output, or
  denial.
- A [Bounded Context](bounded-context.md) bounds Decision visibility and use.
