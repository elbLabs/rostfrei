---
title: Invariant
kind: reference
---

# Invariant

## Definition

An **Invariant** is a named rule that must hold after transition logic produces
an owner's completed candidate state and before that candidate is committed.

An invariant belongs to one [Aggregate](aggregate.md), [Entity](entity.md), or
[Value Object](value-object.md). A [Domain Service](domain-service.md) cannot own
invariants because it owns no state.

## Rust Representation

Invariant checkers are declared on owner-kind contract traits:

```rust
use domain::{
    Aggregate, InvariantOwnerType, InvariantViolation, domain_invariants,
};

#[domain_invariants(aggregate)]
pub(crate) trait ProductStockInvariants {
    #[invariant(id = "stock-nonnegative", label = "Stock is nonnegative")]
    fn stock_nonnegative(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[domain_invariants(aggregate)]
pub(crate) trait ProductReservationInvariants {
    #[invariant(
        id = "reservation-within-stock",
        label = "Reservation is within stock",
    )]
    fn reservation_within_stock(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

#[derive(Aggregate)]
#[domain(
    id = "product",
    label = "Product",
    context = Catalog,
    root = ProductRoot,
    invariants = [ProductStockInvariants, ProductReservationInvariants],
)]
pub struct Product;

impl ProductStockInvariants for Product {
    fn stock_nonnegative(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation> {
        (candidate.stock < 0)
            .then(|| InvariantViolation::new("stock", "must be nonnegative"))
    }
}

impl ProductReservationInvariants for Product {
    fn reservation_within_stock(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation> {
        (candidate.reserved > candidate.stock).then(|| {
            InvariantViolation::new("reserved", "must not exceed stock")
        })
    }
}
```

Every contract names its owner kind explicitly:

- `#[domain_invariants(aggregate)]`
- `#[domain_invariants(entity)]`
- `#[domain_invariants(value_object)]`

There is no Domain Service invariant contract kind. An invariant contract is
non-generic, has inherited or restricted visibility, and has no existing
supertraits. Each checker is an associated function with no receiver or default
body. Its exact contract is:

```rust
fn check(
    candidate: &<Self as InvariantOwnerType>::Candidate,
) -> Option<InvariantViolation>;
```

For an Aggregate, `Candidate` is its configured root. For an Entity or Value
Object, `Candidate` is `Self`. `None` means the rule holds; `Some` returns one
`InvariantViolation`. A checker does not return a [Domain Error](domain-error.md)
or a technical failure.

## Attachment and Validation

Each `invariants = [TraitPath, ...]` entry attaches one implemented contract to
the owner. Implementing a trait does not attach it. Multiple attached traits
form one complete invariant set for that owner; they are not independently
validated sets.

The canonical validation call is:

```rust
<Owner as InvariantOwnerType>::validate_invariants(&candidate)
```

It returns `Ok(())` when every checker holds. Otherwise it returns a nonempty
`Err(Vec<InvariantViolation>)` containing the complete collection. Validation
does not short-circuit: it runs every attached checker in attachment order, then
trait method source order. The collection is therefore complete and
deterministic. An owner with no attached contracts validates successfully.

## Action Execution

Validation is explicit inside the [Action](action.md):

1. Stage the complete candidate produced by transition logic.
2. Validate it with
   `<Owner as InvariantOwnerType>::validate_invariants(&candidate)`.
3. Translate the complete `Vec<InvariantViolation>` into the Action's one
   owner-owned Domain Error.
4. Commit only after validation succeeds.

The compiler generates the owner validator but does not inject validation,
Action wrappers, rollback, violation translation, or commit behavior into
Actions. Staging prevents committed state from needing rollback.

`InvariantViolation` is compiler-neutral validation data containing a path and
reason. The checker returns that data; the Action defines the business denial
exposed by its Domain Error.

## Inventory

The owner derive exposes descriptors for all attached contracts. Registering an
Aggregate, Entity, or Value Object in its normal `domain_model!` owner inventory
automatically projects those descriptors; there is no separate invariant
inventory.

Projection order is Aggregate owners, Entity owners, then Value Object owners.
Within each owner kind it follows model owner order, `invariants` attachment
order, then trait method source order. Projection includes the complete attached
owner set deterministically. It does not execute validation or link invariants
to Actions.

## Boundaries

An invariant does not change or commit state, emit events, inspect another
owner's state, represent technical failure, or translate itself into a Domain
Error.
