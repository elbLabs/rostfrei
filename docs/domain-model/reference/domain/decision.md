---
title: Decision
kind: reference
---

# Decision

## Definition

A **Decision** is a named, pure, stateless domain policy owned by an
[Aggregate](aggregate.md) or [Entity](entity.md). It is an associated function on
that owner's inherent impl. Its parameters contain every fact required to
evaluate the policy, and its non-generic outcome enum describes the closed set
of possible domain results.

A Decision does not classify those results as accepted, denied, success, or
failure. A caller assigns meaning to an outcome in the context of its own
behavior.

## Ownership and Groups

A Decision belongs to exactly one Aggregate or Entity. Value Objects and Domain
Services cannot own Decisions.

Decisions are organized into explicit groups. A group is a user-declared,
non-generic Rust marker type associated with one inherent Decision impl:

```rust
pub(crate) struct AssignmentPolicies;
pub(crate) struct SchedulingPolicies;
```

The marker is an ordinary Rust type. Its declaration controls its visibility
and module path; `domain_decisions` has no visibility option. Decision methods
also use normal Rust visibility.

The owner explicitly attaches groups in projection order:

```rust
#[derive(Aggregate)]
#[domain(
    id = "todo",
    label = "Todo",
    context = Planning,
    root = TodoRoot,
    decisions = [AssignmentPolicies, SchedulingPolicies],
)]
pub struct Todo;
```

Each attachment must be a normal, non-generic type path whose group names that
exact owner. Implementing a group does not attach it. Omitting `decisions` is
equivalent to `decisions = []`; the former bare `decisions` marker is not
supported.

An owner may attach multiple groups. Compiled Decisions preserve model owner
inventory order, group attachment order, and method source order within each
group. Groups organize Rust declarations and attachment only: they have no model
ID and are not projected into compiled JSON.

Ownership identifies and projects the Decision, but does not restrict it to
Actions owned by the same object. Any [Action](action.md) in the same
[Bounded Context](bounded-context.md) may call a Decision when ordinary Rust
visibility makes its function accessible. The compiler validates declaration
and attachment; it does not enforce Action-to-Decision call permissions.

## Rust Representation

A Decision group is declared on an inherent owner impl. The owner kind and group
are both explicit:

```rust
#[derive(DecisionOutcome)]
pub(crate) enum AssignmentOutcome {
    #[outcome(id = "assignable", label = "Assignable")]
    Assignable { remaining_capacity: u32 },

    #[outcome(id = "requires-review", label = "Requires review")]
    RequiresReview(ReviewReason),

    #[outcome(id = "unavailable", label = "Unavailable")]
    Unavailable,
}

#[domain_decisions(aggregate, group = AssignmentPolicies)]
impl Todo {
    #[decision(id = "can-assign", label = "Can assign todo")]
    pub(crate) fn can_assign(
        assignee: &Assignee,
        open_assignment_count: u32,
    ) -> AssignmentOutcome {
        if assignee.requires_review() {
            AssignmentOutcome::RequiresReview(assignee.review_reason())
        } else if open_assignment_count >= 10 {
            AssignmentOutcome::Unavailable
        } else {
            AssignmentOutcome::Assignable {
                remaining_capacity: 9 - open_assignment_count,
            }
        }
    }
}
```

Use `#[domain_decisions(aggregate, group = GroupType)]` for an Aggregate owner
and `#[domain_decisions(entity, group = GroupType)]` for an Entity owner. The
macro accepts no group visibility syntax; declare `GroupType` with the desired
normal Rust visibility.

Every Decision method has exactly one
`#[decision(id = "...", label = "...")]`. The `id` is stable and local to the
owner; the `label` is human-readable. IDs must remain unique across all groups
attached to one owner.

The decorated impl is non-generic and inherent. A Decision is an associated
function with no `self` receiver or Aggregate root parameter. It may have zero
or more parameters, each using a simple immutable identifier and a supported
scalar or [Value Object](value-object.md) type.

A parameter may own its value as `T` or borrow it as a top-level immutable `&T`.
Both forms produce the same model metadata for `T`. An immutable borrow uses an
elided lifetime or explicit `'static`; Decisions cannot declare named lifetime
generics. Mutable references such as `&mut T` and nested references such as
`Option<&T>`, `Vec<&T>`, or `&&T` are rejected.

A Decision signature does not accept:

- `&self`, `&mut self`, or `self`
- an Aggregate root
- mutable or nested references
- [Domain Commands](domain-command.md)
- [Domain Events](domain-event.md)
- [Domain Errors](domain-error.md)
- top-level `Option` or `Vec`

Decision functions cannot be async, unsafe, extern, variadic, or generic, and
cannot have a `where` clause.

## Outcome Contract

Every Decision declares an explicit, owned return type. It is a non-generic enum
deriving `DecisionOutcome`; it is not `Result` and cannot contain references.

Each enum variant has exactly one stable
`#[outcome(id = "...", label = "...")]`. Outcome IDs are unique within the enum.
The derive accepts all Rust enum variant shapes:

- unit, such as `Unavailable`
- tuple, such as `RequiresReview(ReviewReason)`
- named struct, such as `Assignable { remaining_capacity: u32 }`

Tuple and named fields may be supported scalars or Value Objects. References,
containers, Domain Errors, Domain Events, Entities, and arbitrary unmodeled Rust
types are not outcome field values. Explicit enum discriminants are unsupported.
Variant and field order follow source order; raw named-field identifiers are
normalized.

Each projected variant is a first-class, Decision-scoped outcome with an ID of
the form `DecisionOutcomeId { decision, local }`. The Rust enum itself is the
closed return contract; the compiled model nests its outcomes under the
Decision rather than creating a group or standalone outcome inventory.

There is no accepted/denied classification. Unit, tuple, and struct describe
payload shape only. No variant is automatically a Domain Error or Domain Event.

## Calls and Translation

Calls use ordinary inherent Rust syntax and preserve the authored enum:

```rust
match Todo::can_assign(&assignee, open_assignment_count) {
    AssignmentOutcome::Assignable { remaining_capacity } => {
        plan_assignment(remaining_capacity);
    }
    AssignmentOutcome::RequiresReview(reason) => {
        request_manual_review(reason);
    }
    AssignmentOutcome::Unavailable => {
        return Err(AssignmentDenied::capacity_reached());
    }
}
```

Rust module and method visibility determine whether the call is available. The
compiled model contains no Action-to-Decision links, and `#[action(...)]` has no
Decision metadata. The compiler does not infer calls or matches from function
bodies.

An [Action](action.md) exhaustively matches the outcome and translates it for
its own contract as needed. A variant may cause the Action to continue, raise a
[Domain Event](domain-event.md), return an owner-appropriate
[Domain Error](domain-error.md), or perform another allowed domain step. That
translation does not classify the Decision outcome globally and does not add an
Action-to-Decision model relationship.

## Purity and Statelessness

A Decision is normatively pure and stateless. It does not mutate domain state,
load state or infrastructure data, invoke Actions, emit events, or depend on
hidden mutable state. Equal modeled facts produce the same outcome.

The compiler validates the declared signature and modeled types. It does not
mechanically inspect the method body for I/O, mutation through external state,
nondeterminism, or other side effects. Authors preserve these semantics.

## Domain Test References

Decision Domain Tests use readable owner-associated syntax derived from the
stable Decision ID:

```rust
#[domain_decision_test(Todo::CAN_ASSIGN)]
fn full_queues_are_unavailable() {
    // ...
}
```

`Owner::REFERENCE` is attribute syntax backed by a doc-hidden,
group-typed `DecisionReference<Group>` anchor. The Domain Test macro checks that
the referenced group is attached to that exact owner before producing the
Decision ID. A wrong owner, unknown stable reference, unattached group, or group
attached to a different owner fails compilation. Application calls still use
the actual function name, such as `Todo::can_assign(...)`.

## Consumers

Actions are the supported modeled Decision consumer. Any visible Action in the
same Bounded Context may call a Decision through ordinary Rust.

Decision integration with [Invariants](invariant.md) and
[Lifecycles](lifecycle.md) is not defined. Their descriptors do not reference
Decisions, and this reference does not define an execution relationship for
them.

## Model Shape

The attached group projects one descriptor per Decision. The group itself is
not part of the descriptor:

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
    outcomes: &[
        DecisionOutcomeDescriptor {
            local_id: "assignable",
            label: "Assignable",
            shape: DecisionOutcomeShapeDescriptor::Struct {
                fields: &[DecisionOutcomeNamedFieldDescriptor {
                    name: "remaining_capacity",
                    value: DecisionOutcomeValueDescriptor::Scalar(ScalarType::U32),
                }],
            },
        },
        DecisionOutcomeDescriptor {
            local_id: "requires-review",
            label: "Requires review",
            shape: DecisionOutcomeShapeDescriptor::Tuple {
                fields: &[DecisionOutcomeValueDescriptor::ValueObject(
                    ReviewReason::DESCRIPTOR.id,
                )],
            },
        },
        DecisionOutcomeDescriptor {
            local_id: "unavailable",
            label: "Unavailable",
            shape: DecisionOutcomeShapeDescriptor::Unit,
        },
    ],
    implementation: DecisionImplementationDescriptor::Rust,
}
```

Borrowing `assignee` as `&Assignee` does not appear in the descriptor; an owned
`Assignee` parameter projects the same `DecisionInputDescriptor`.

Compiled model JSON uses the top-level `decisions` inventory and ordered nested
`outcomes`:

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
      "outcomes": [
        {
          "id": {
            "decision": {
              "owner": {
                "kind": "aggregate",
                "id": { "context": "planning", "local": "todo" }
              },
              "local": "can-assign"
            },
            "local": "assignable"
          },
          "label": "Assignable",
          "shape": {
            "kind": "struct",
            "fields": [
              {
                "name": "remaining_capacity",
                "value": { "kind": "scalar", "scalar": "u32" }
              }
            ]
          }
        },
        {
          "id": {
            "decision": {
              "owner": {
                "kind": "aggregate",
                "id": { "context": "planning", "local": "todo" }
              },
              "local": "can-assign"
            },
            "local": "requires-review"
          },
          "label": "Requires review",
          "shape": {
            "kind": "tuple",
            "fields": [
              {
                "kind": "valueObject",
                "id": {
                  "owner": {
                    "kind": "aggregate",
                    "id": { "context": "planning", "local": "todo" }
                  },
                  "local": "review-reason"
                }
              }
            ]
          }
        },
        {
          "id": {
            "decision": {
              "owner": {
                "kind": "aggregate",
                "id": { "context": "planning", "local": "todo" }
              },
              "local": "can-assign"
            },
            "local": "unavailable"
          },
          "label": "Unavailable",
          "shape": { "kind": "unit" }
        }
      ],
      "implementation": { "kind": "rust" }
    }
  ]
}
```

`parameters` and `outcomes` are always arrays and preserve declaration order.
Tuple shapes contain ordered value descriptors. Struct shapes contain ordered
`name`/`value` field descriptors. Unit shapes contain only `kind`.

Decision JSON has no `output`, `error`, accepted/denied flag, group, consumer,
Action, gate, command, event, implementation path, or DMN field.

## Implementation Status

Inherent owner functions, explicit group attachment, `DecisionOutcome` enums,
and group-typed Domain Test references are the Rust implementation. DMN remains
a future implementation target; the current model does not load a DMN file or
project a DMN requirements graph.

## Related Concepts

- [ADR 0016](../../../adr/0016-decision-policies-groups-and-outcomes.md) records
  the Decision redesign and migration from `output`/`error` model fields.
- An [Action](action.md) calls a Decision and translates its outcomes when needed.
- A [Value Object](value-object.md) can define a Decision parameter or an outcome
  payload field.
- A [Domain Error](domain-error.md) belongs to an Action owner, not to a Decision
  outcome.
- A [Bounded Context](bounded-context.md) bounds Decision visibility and use.
