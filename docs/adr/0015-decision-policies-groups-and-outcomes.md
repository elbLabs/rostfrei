# ADR 0015: Decision policies, groups, and outcomes

## Status

Accepted.

The reusable-rule vocabulary in this historical decision is superseded by
[ADR 0036](0036-domain-policy-vocabulary.md): Decision remains the
command-scoped result, while the capability described here is a Domain Policy.

The parameter and outcome-payload modeling described by the original decision
is superseded by [ADR 0024](0024-semantic-decision-outcomes-and-ordinary-payloads.md).
Decision groups, attachments, owner metadata, projection, and generated test
references are superseded by
[ADR 0032](0032-singular-decisions-invariants-and-tests.md).

## Context

[ADR 0013](0013-compiled-domain-model.md) established Decisions as inherent
Aggregate- or Entity-owned Rust behavior in the compiled domain model. The initial contract allowed one attached
Decision block per owner, modeled `Result<T, E>` as output and error, and treated
its two branches as accepted and denied.

That shape imposed an Action-oriented classification on reusable domain policy,
made one impl block carry both organization and attachment, and could not model a
closed set of equally meaningful business outcomes. It also rejected immutable
borrows even though borrowing changes Rust call mechanics rather than domain
metadata.

This ADR refines the Decision-specific parts of ADR 0013. It does not change
Action, command-execution, or Domain Error result contracts.

## Decision

A Decision is a pure, stateless domain policy owned by exactly one Aggregate or
Entity and implemented as an associated function on that owner's inherent impl.
It has no receiver and does not read owner state implicitly. Callers supply all
facts as parameters.

Decision impls are organized by user-declared, non-generic Rust marker types:

```rust
pub(crate) struct AssignmentPolicies;

#[domain_decisions(aggregate, group = AssignmentPolicies)]
impl TodoAggregate {
    #[decision(id = "can-assign", label = "Can assign todo")]
    pub(crate) fn can_assign(assignee: &Assignee) -> AssignmentOutcome {
        todo!()
    }
}
```

The marker is an ordinary Rust type. Its declaration provides normal Rust
visibility and module placement. `domain_decisions` has no visibility option.
Decision functions likewise use ordinary Rust function visibility.

An Entity explicitly attaches zero or more groups in order. Attachment
establishes ownership and projection, and a group must name the exact Entity it
is attached to. Projection preserves owner inventory order, group attachment
order, and Decision source order within each group. Groups are an organizational
Rust mechanism and are not projected into compiled model JSON.

[ADR 0020](0020-aggregate-definition-and-event-set.md) subsequently removed
Aggregate decision attachment. Aggregate-owned Decision contracts remain
ordinary typed Rust behavior, but are not projected into the compiled model
until Rostfrei has a relationship it can derive or validate without a manual
group list.

Decision parameters are ordinary Rust inputs. Rostfrei does not classify or
project their types; the authored function signature is authoritative.
Decisions have no receiver, Aggregate root parameter, or hidden state input.

Every Decision declares an explicit owned return type. That type is a
non-generic enum deriving `DecisionOutcome`, not `Result`:

```rust
#[derive(DecisionOutcome)]
pub enum AssignmentOutcome {
    #[outcome(id = "assignable", label = "Assignable")]
    Assignable { remaining_capacity: u32 },

    #[outcome(id = "requires-review", label = "Requires review")]
    RequiresReview(ReviewReason),

    #[outcome(id = "unavailable", label = "Unavailable")]
    Unavailable,
}
```

Every variant has one stable `#[outcome(id = "...", label = "...")]`. Variants
may be unit, tuple, or named struct shapes with arbitrary ordinary Rust payload
types. Only outcome IDs, labels, and source order are domain metadata; payload
shape remains an implementation detail of the Rust enum.

Each variant is projected as a first-class outcome whose ID is scoped to its
Decision. The model does not classify variants as accepted, denied, success, or
failure. Those meanings belong to the caller's use of a policy result, not to
the Decision contract.

Decision calls remain ordinary inherent Rust calls. Callers exhaustively match
the returned enum and decide what the result means in their own behavior. An
Action may continue, raise one or more Domain Events, or translate a relevant
variant into its owner-appropriate Domain Error. The compiler records no
Action-to-Decision call graph or translation metadata.

Domain Tests retain readable owner-associated subject syntax:

```rust
#[domain_decision_test(TodoAggregate::CAN_ASSIGN)]
fn full_queues_are_unavailable() {
    // ...
}
```

The macro resolves that syntax to a hidden `DecisionReference<Group>` generated
from the stable Decision ID. The group type preserves exact attachment checking:
the test compiles only when that exact group is attached to that exact owner.
The hidden anchor is implementation machinery, not public application syntax.

Compiled Decision JSON contains an ordered `outcomes` array. Each outcome
contains only its Decision-scoped stable ID and label. Decision parameters,
payload shapes, group names, and attachments are not projected.

## Consequences

Decision contracts describe domain alternatives without imposing Action denial
semantics. Adding a modeled alternative is an enum change and therefore makes
ordinary exhaustive matches identify callers that require translation updates.

Group markers permit multiple cohesive Decision impls per owner and stable,
explicit projection order while retaining standard Rust module privacy. Moving
a Decision between attached groups does not change its model ID, but can change
projection order and the hidden reference's group type; Domain Tests continue to
verify exact attachment.

Input signatures have no compiled-model representation, so authoring can use
the Rust types and borrowing strategy appropriate to the implementation.

Outcome enums are closed and non-generic, while their payload fields may use
arbitrary Rust types. Actions remain responsible for translating policy
outcomes into their own errors and events.

Compiled-model consumers must not expect Decision parameters, outcome payload
shapes, or Decision group data in the model.
