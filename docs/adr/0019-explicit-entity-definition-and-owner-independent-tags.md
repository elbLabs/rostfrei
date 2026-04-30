# ADR 0019: Explicit entity definitions and owner-independent tags

## Status

Accepted.

## Context

The original `Entity` derive combined semantic identity with executable owner
and identity types, plus manually maintained action, decision, invariant, and
lifecycle attachments. Lifecycle and invariant declarations also repeated an
owner even though their metadata is useful independently of an entity or
aggregate attachment.

Those lists were descriptive claims rather than relationships Rust could prove
from behavior. They also made compiled-model projection depend on implicit
capabilities declared inside a derive attribute.

## Decision

`#[derive(Entity)]` declares an entity's local ID, label, fields, and identity
field only. An ordinary `EntityDefinition` implementation supplies the
compiler-checked owner and identity relationships:

```rust
#[derive(Entity)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    #[domain(identity)]
    bicycle_id: BicycleId,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;
}
```

The typed filesystem convention requires the matching `EntityDefinition`
implementation beside the `Entity` declaration in `entity.rs` or `root.rs`.

Entity derives do not accept action, decision, invariant, or lifecycle
attachments. Those contracts remain ordinary Rust code, but they are not
implicitly projected as entity capabilities in the compiled domain model.
`DecisionOutcome` remains the explicit metadata contract for decision results.

Entity lifecycles are owner-independent ordered state vocabularies:

```rust
#[derive(EntityLifecycle)]
#[domain(id = "rental-status", label = "Bicycle rental status")]
pub enum BicycleRentalLifecycle {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}
```

Their descriptors contain the lifecycle ID, label, and ordered state IDs and
labels. They do not declare an owner, initial state, transitions, or action
references, and an entity does not automatically project a lifecycle.

Invariant contracts are likewise owner-independent metadata tags:

```rust
#[domain_invariants]
trait FleetConsistency {
    #[invariant(id = "unique-bicycle-identities", label = "Bicycle identities are unique")]
    fn unique_bicycle_identities(candidate: &RentalFleet) -> Option<InvariantViolation>;
}
```

The annotated method's Rust signature and implementation express validation
behavior. The macro provides stable descriptors and references only; it does
not impose a candidate type, attach the contract to an owner, fan validation
out automatically, or project invariants into the compiled model.

## Consequences

This is a breaking source change. Applications must remove `owner`, `actions`,
`decisions`, `invariants`, and `lifecycle` from entity attributes and add a
matching `EntityDefinition` implementation. Lifecycle declarations must remove
owner, initial-state, and transition metadata and use `#[state(...)]` on
variants. Invariant contracts must use argument-free `#[domain_invariants]` and
ordinary method signatures.

[ADR 0030](0030-singular-decisions-invariants-and-tests.md) subsequently
replaces plural invariant contracts with one `#[domain_invariant(id, label)]`
trait and a direct aggregate/entity implementation.

Executable relationships remain compiler checked where Rust has an explicit
type contract. Descriptive vocabularies remain reusable without claiming an
attachment that the framework cannot prove. Compiled domain models no longer
imply entity capabilities, invariant inventories, or lifecycle projection.

The identity side of `EntityDefinition` is refined by
[ADR 0020](0020-slim-domain-identities.md), which makes `DomainIdentity` a
metadata-free marker discovered through its entity rather than a separate
compiled-model inventory.

Value-object declarations and operation-specific DTOs are separated by
[ADR 0021](0021-slim-value-objects-and-ordinary-dtos.md). Semantic value
objects retain ID and label metadata, while action inputs and query outputs are
ordinary Rust types with no inferred model shape.
