---
title: Invariant Validation
kind: handbook
---

# Invariant Validation

Invariant validation checks the completed candidate produced by transition
logic before that candidate is committed.

```text
Current State
  -> Stage Completed Candidate
  -> Explicit Owner Validation
  -> Translate Complete Violations to One Owner-Owned Domain Error
  -> Commit on Success
```

## Declare and Attach

Declare checkers on a contract trait for the owner kind:

```rust
#[domain_invariants(entity)]
trait LineInvariants {
    #[invariant(id = "positive-quantity", label = "Quantity is positive")]
    fn positive_quantity(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[derive(Entity)]
#[domain(
    id = "line",
    label = "Line",
    owner = Order,
    invariants = [LineInvariants, LinePricingInvariants],
)]
struct Line {
    #[domain(identity)]
    id: LineId,
    quantity: i32,
    unit_price_cents: u64,
}

#[derive(DomainIdentity)]
#[domain(owner = Line)]
struct LineId(String);
```

Use `#[domain_invariants(aggregate)]`, `#[domain_invariants(entity)]`, or
`#[domain_invariants(value_object)]`. A
[Domain Service](../reference/domain/domain-service.md) cannot own or attach
invariant contracts.

Each `invariants = [...]` entry attaches one implemented trait. Multiple traits
form one complete invariant set for the owner. Omitting `invariants`, or using
`invariants = []`, gives the owner an empty set that validates successfully.

Every checker returns `Option<InvariantViolation>`: `None` when its rule holds,
or one `InvariantViolation` when it fails. Checkers do not return Domain Errors.
The generated owner validator runs every checker without short-circuiting and
returns either `Ok(())` or a nonempty, complete `Vec<InvariantViolation>`.
Ordering is deterministic: attachment order first, then trait method source
order.

## Action Pattern

An Action stages rather than commits state before validation:

```rust
let mut candidate = current.clone();
candidate.apply(change);

<Product as InvariantOwnerType>::validate_invariants(&candidate)
    .map_err(ProductError::InvalidState)?;

*current = candidate;
```

The `map_err` step translates the complete violation collection into the
Action's owner-owned [Domain Error](../reference/domain/domain-error.md). The
Domain Error is the modeled denial; each `InvariantViolation` is
compiler-neutral path-and-reason data returned by a checker. Technical failures
are outside the checker contract.

The sequence remains explicit:

1. Stage the completed candidate.
2. Validate through the canonical
   `<Owner as InvariantOwnerType>::validate_invariants` call.
3. Translate all returned violations into the Action's Domain Error.
4. Commit only on validation success.

The compiler generates validation machinery but injects no validation, Action
wrapper, rollback, translation, or commit behavior into Actions. Staging is what
keeps failed validation from requiring rollback.

## Inventory

Register invariant owners through the normal `domain_model!` `aggregates`,
`entities`, and `value_objects` inventories. Their attached invariant contracts
are discovered and projected automatically; no separate invariant inventory is
required.

Automatic projection follows Aggregate, Entity, then Value Object owner order.
For each owner it preserves `invariants` attachment order and trait method source
order, producing a complete deterministic `invariants` collection. Projection
does not execute validation or infer Action linkage.

## Boundaries

Invariant validation does not commit state, emit events, validate another owner,
or provide persistence or reconstitution behavior. Domain Services do not own
invariant sets.
